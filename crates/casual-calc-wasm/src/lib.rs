//! `casual-calc-wasm` — the `wasm-bindgen` bridge for the browser demo & editor.
//!
//! A thin transport over the host-agnostic engine (the same core runs native on
//! Tauri). Two surfaces:
//!
//! - **Stateless helpers** (`eval_formula`, `render_xlsx`, `describe_xlsx`) for
//!   the landing page.
//! - **A live editor session** kept in a thread-local [`WorkbookSession`]:
//!   open/edit/undo/redo/save, and query the visible cells as JSON so the browser
//!   can draw the grid on a canvas (text is rendered by the browser; the engine
//!   supplies positions + display strings). See `docs/02-ARCHITECTURE.md`.

use std::cell::RefCell;

use casual_calc_eval::recalculate;
use casual_calc_formula::parse;
use casual_calc_import::import_package;
use casual_calc_layout::{
    DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, display_text, layout_viewport,
};
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook};
use casual_calc_render::render_png;
use casual_calc_sdk::{EditOperation, WorkbookSession};
use wasm_bindgen::prelude::*;

thread_local! {
    static SESSION: RefCell<Option<WorkbookSession>> = const { RefCell::new(None) };
}

/// The engine version string.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Default column width in device pixels at 96 dpi (for the canvas grid).
#[wasm_bindgen]
pub fn default_col_px() -> u32 {
    (DEFAULT_COL_WIDTH * 96 / 1440) as u32
}

/// Default row height in device pixels at 96 dpi.
#[wasm_bindgen]
pub fn default_row_px() -> u32 {
    (DEFAULT_ROW_HEIGHT * 96 / 1440) as u32
}

// ---------------------------------------------------------------------------
// Stateless landing-page helpers.
// ---------------------------------------------------------------------------

/// Evaluate a single self-contained formula (e.g. `=1+2*3`, `=SUM(1,2,3)`).
#[wasm_bindgen]
pub fn eval_formula(input: &str) -> String {
    let body = input.trim().strip_prefix('=').unwrap_or(input.trim());
    let expr = match parse(body) {
        Ok(expr) => expr,
        Err(err) => return err.to_string(),
    };
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Demo");
    let handle = workbook.store_formula(expr);
    let mut cell = Cell::value(CellValue::Empty);
    cell.formula = Some(handle);
    sheet.cells.set(CellRef::new(0, 0), cell);
    workbook.sheets.push(sheet);
    recalculate(&mut workbook);
    let value = workbook.sheets[0]
        .cells
        .get(CellRef::new(0, 0))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty);
    value_text(&workbook, &value)
}

/// Open an `.xlsx` and render a viewport of the first sheet to PNG bytes.
#[wasm_bindgen]
pub fn render_xlsx(
    bytes: &[u8],
    width_px: u32,
    height_px: u32,
    dpi: u32,
) -> Result<Vec<u8>, JsError> {
    let outcome = import_package(bytes.to_vec()).map_err(js)?;
    let mut workbook = outcome.workbook;
    recalculate(&mut workbook);
    let geometry = workbook
        .sheets
        .first()
        .map(GridGeometry::for_sheet)
        .unwrap_or_default();
    let viewport = viewport_px(width_px, height_px, dpi);
    let list = layout_viewport(&workbook, 0, &geometry, &viewport);
    render_png(&list, &geometry, &viewport, dpi).map_err(js)
}

/// A short summary of an opened `.xlsx`.
#[wasm_bindgen]
pub fn describe_xlsx(bytes: &[u8]) -> Result<String, JsError> {
    let outcome = import_package(bytes.to_vec()).map_err(js)?;
    let wb = outcome.workbook;
    let (name, cells) = wb
        .sheets
        .first()
        .map(|s| (s.name.clone(), s.cells.len()))
        .unwrap_or_default();
    Ok(format!(
        "{} sheet(s); \"{name}\" has {cells} populated cell(s)",
        wb.sheets.len()
    ))
}

// ---------------------------------------------------------------------------
// Editor session.
// ---------------------------------------------------------------------------

/// Start a new blank session with one sheet.
#[wasm_bindgen]
pub fn session_new() {
    let mut session = WorkbookSession::blank();
    session
        .workbook_mut()
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(0x5348, 1)), "Sheet1"));
    set_session(session);
}

/// Open an `.xlsx` into the editor session.
#[wasm_bindgen]
pub fn session_open(bytes: &[u8]) -> Result<(), JsError> {
    let session = WorkbookSession::open(bytes.to_vec()).map_err(js)?;
    set_session(session);
    Ok(())
}

/// The sheet names as a JSON array of strings.
#[wasm_bindgen]
pub fn session_sheet_names() -> String {
    with_session(|s| {
        let names: Vec<String> = s
            .workbook()
            .sheets
            .iter()
            .map(|sheet| json_string(&sheet.name))
            .collect();
        format!("[{}]", names.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The used bounds of a sheet as JSON `{ "rows": n, "cols": n }` (counts).
#[wasm_bindgen]
pub fn session_used_bounds(sheet: usize) -> String {
    with_session(|s| {
        let mut rows = 0u32;
        let mut cols = 0u32;
        if let Some(sheet) = s.workbook().sheets.get(sheet) {
            for (at, _) in sheet.cells.iter() {
                rows = rows.max(at.row + 1);
                cols = cols.max(at.col + 1);
            }
        }
        format!("{{\"rows\":{rows},\"cols\":{cols}}}")
    })
    .unwrap_or_else(|| "{\"rows\":0,\"cols\":0}".to_owned())
}

/// Column widths in device pixels (96 dpi) for `count` columns starting at
/// `first`, as a JSON array. Lets the editor draw real `.xlsx` column widths.
#[wasm_bindgen]
pub fn session_col_px(sheet: usize, first: u32, count: u32) -> String {
    axis_px(sheet, first, count, DEFAULT_COL_WIDTH, true)
}

/// Row heights in device pixels (96 dpi) for `count` rows starting at `first`.
#[wasm_bindgen]
pub fn session_row_px(sheet: usize, first: u32, count: u32) -> String {
    axis_px(sheet, first, count, DEFAULT_ROW_HEIGHT, false)
}

/// Shared body of [`session_col_px`]/[`session_row_px`]: a JSON array of
/// per-line pixel sizes, honoring the sheet's overrides and default.
fn axis_px(sheet: usize, first: u32, count: u32, fallback: i64, columns: bool) -> String {
    with_session(|s| {
        let sizing = s
            .workbook()
            .sheets
            .get(sheet)
            .map(|sh| if columns { &sh.columns } else { &sh.rows });
        let mut out = String::from("[");
        for i in 0..count {
            if i > 0 {
                out.push(',');
            }
            let twips = sizing.map_or(fallback, |sz| sz.size(first + i, fallback));
            out.push_str(&(twips * 96 / 1440).to_string());
        }
        out.push(']');
        out
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Absolute pixel offset (96 dpi) of a column's left edge from column 0.
#[wasm_bindgen]
pub fn session_col_offset_px(sheet: usize, col: u32) -> i32 {
    with_session(|s| geometry_of(s, sheet).columns.offset(col) as i32 * 96 / 1440).unwrap_or(0)
}

/// Absolute pixel offset (96 dpi) of a row's top edge from row 0.
#[wasm_bindgen]
pub fn session_row_offset_px(sheet: usize, row: u32) -> i32 {
    with_session(|s| geometry_of(s, sheet).rows.offset(row) as i32 * 96 / 1440).unwrap_or(0)
}

/// The column containing absolute pixel position `px` (clamped at 0).
#[wasm_bindgen]
pub fn session_col_at_px(sheet: usize, px: i32) -> u32 {
    with_session(|s| {
        geometry_of(s, sheet)
            .columns
            .line_at(px.max(0) as i64 * 1440 / 96)
    })
    .unwrap_or(0)
}

/// The row containing absolute pixel position `px` (clamped at 0).
#[wasm_bindgen]
pub fn session_row_at_px(sheet: usize, px: i32) -> u32 {
    with_session(|s| {
        geometry_of(s, sheet)
            .rows
            .line_at(px.max(0) as i64 * 1440 / 96)
    })
    .unwrap_or(0)
}

/// Set a column's width to `px` device pixels (undoable).
#[wasm_bindgen]
pub fn session_set_col_width(sheet: usize, col: u32, px: u32) -> Result<(), JsError> {
    edit_axis(
        sheet,
        EditOperation::SetColumnWidth {
            sheet,
            col,
            width: Some(resize_px_to_twips(px)),
        },
    )
}

/// Set a row's height to `px` device pixels (undoable).
#[wasm_bindgen]
pub fn session_set_row_height(sheet: usize, row: u32, px: u32) -> Result<(), JsError> {
    edit_axis(
        sheet,
        EditOperation::SetRowHeight {
            sheet,
            row,
            height: Some(resize_px_to_twips(px)),
        },
    )
}

/// Clear a column's explicit width, reverting it to the sheet default (undoable).
#[wasm_bindgen]
pub fn session_clear_col_width(sheet: usize, col: u32) -> Result<(), JsError> {
    edit_axis(
        sheet,
        EditOperation::SetColumnWidth {
            sheet,
            col,
            width: None,
        },
    )
}

/// Clear a row's explicit height, reverting it to the sheet default (undoable).
#[wasm_bindgen]
pub fn session_clear_row_height(sheet: usize, row: u32) -> Result<(), JsError> {
    edit_axis(
        sheet,
        EditOperation::SetRowHeight {
            sheet,
            row,
            height: None,
        },
    )
}

/// Convert device pixels (96 dpi) to twips, floored at a sensible minimum so a
/// column/row can never be dragged to zero and vanish.
fn resize_px_to_twips(px: u32) -> i64 {
    (px.max(8) as i64) * 1440 / 96
}

/// The grid geometry (column widths / row heights) of a sheet.
fn geometry_of(s: &WorkbookSession, sheet: usize) -> GridGeometry {
    s.workbook()
        .sheets
        .get(sheet)
        .map(GridGeometry::for_sheet)
        .unwrap_or_default()
}

fn edit_axis(_sheet: usize, op: EditOperation) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.edit(op).map_err(js)
    })
}

/// Visible cells in `[first_row..=last_row] × [first_col..=last_col]` as a JSON
/// array of `{ r, c, t, a }` (row, col, display text, align: "l"|"r").
#[wasm_bindgen]
pub fn session_cells(
    sheet: usize,
    first_row: u32,
    first_col: u32,
    last_row: u32,
    last_col: u32,
) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sheet) = wb.sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let mut items = Vec::new();
        for (at, cell) in sheet.cells.row_band(first_row, last_row) {
            if at.col < first_col || at.col > last_col {
                continue;
            }
            let text = display_text(wb, cell);
            let style = cell.style.and_then(|id| wb.styles.get(id));
            let fill = style.and_then(|s| s.fill_color.clone()).unwrap_or_default();
            if text.is_empty() && fill.is_empty() {
                continue;
            }
            let align = match cell.value {
                CellValue::Number(_) | CellValue::Bool(_) | CellValue::Error(_) => "r",
                _ => "l",
            };
            let mut extra = String::new();
            if style.is_some_and(|s| s.bold) {
                extra.push_str(",\"b\":1");
            }
            if style.is_some_and(|s| s.italic) {
                extra.push_str(",\"i\":1");
            }
            if let Some(fc) = style.and_then(|s| s.font_color.as_deref()) {
                extra.push_str(&format!(",\"fc\":{}", json_string(fc)));
            }
            if !fill.is_empty() {
                extra.push_str(&format!(",\"bg\":{}", json_string(&fill)));
            }
            items.push(format!(
                "{{\"r\":{},\"c\":{},\"t\":{},\"a\":\"{align}\"{extra}}}",
                at.row,
                at.col,
                json_string(&text)
            ));
        }
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
        {
            return format!("={expr}");
        }
        value_text(wb, &cell.value)
    })
    .unwrap_or_default()
}

/// Set a cell from user input (a formula `=…`, a number, or text), then recalc.
#[wasm_bindgen]
pub fn session_set_cell(sheet: usize, row: u32, col: u32, input: &str) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let op = build_set_op(session, sheet, CellRef::new(row, col), input);
        session.edit(op).map_err(js)
    })
}

/// Apply bold + an optional solid fill (`RRGGBB` hex, empty for none) to a cell,
/// preserving its value/formula and number format.
#[wasm_bindgen]
pub fn session_set_style(
    sheet: usize,
    row: u32,
    col: u32,
    bold: bool,
    fill: &str,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let at = CellRef::new(row, col);
        let number_format = session
            .workbook()
            .sheets
            .get(sheet)
            .and_then(|s| s.cells.get(at))
            .and_then(|c| c.style)
            .and_then(|id| session.workbook().styles.get(id))
            .and_then(|s| s.number_format.clone());
        let style = Style {
            number_format,
            bold,
            italic: false,
            font_color: None,
            fill_color: (!fill.is_empty()).then(|| fill.to_owned()),
        };
        let id = session.workbook_mut().intern_style(style);
        session
            .edit(EditOperation::SetStyle {
                sheet,
                at,
                style: Some(id),
            })
            .map_err(js)
    })
}

/// Whether every cell in a range is bold (used for the toolbar toggle state).
#[wasm_bindgen]
pub fn session_range_bold(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> bool {
    with_session(|s| {
        let mut any = false;
        for r in r0..=r1 {
            for c in c0..=c1 {
                let bold = s
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|sh| sh.cells.get(CellRef::new(r, c)))
                    .and_then(|cell| cell.style)
                    .and_then(|id| s.workbook().styles.get(id))
                    .is_some_and(|st| st.bold);
                if !bold {
                    return false;
                }
                any = true;
            }
        }
        any
    })
    .unwrap_or(false)
}

/// Toggle bold across a range (one undo step).
#[wasm_bindgen]
pub fn session_toggle_bold(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let target = !session_range_bold(sheet, r0, c0, r1, c1);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.bold = target)
}

/// Set (or clear, with empty hex) the solid fill across a range (one undo step).
#[wasm_bindgen]
pub fn session_set_fill(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    hex: &str,
) -> Result<(), JsError> {
    let fill = (!hex.is_empty()).then(|| hex.to_owned());
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.fill_color = fill.clone()
    })
}

/// Clear every cell in a range (one undo step).
#[wasm_bindgen]
pub fn session_clear_range(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut ops = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                ops.push(EditOperation::ClearCell {
                    sheet,
                    at: CellRef::new(r, c),
                });
            }
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Copy a range as tab-separated text (for the clipboard).
#[wasm_bindgen]
pub fn session_copy_tsv(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return String::new();
        };
        let mut out = String::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if c > c0 {
                    out.push('\t');
                }
                if let Some(cell) = sh.cells.get(CellRef::new(r, c)) {
                    out.push_str(&display_text(wb, cell));
                }
            }
            out.push('\n');
        }
        out
    })
    .unwrap_or_default()
}

/// Paste tab/newline-separated text starting at a cell (one undo step).
#[wasm_bindgen]
pub fn session_paste_tsv(sheet: usize, row: u32, col: u32, tsv: &str) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut ops = Vec::new();
        for (dr, line) in tsv.split('\n').enumerate() {
            if line.is_empty() && dr > 0 {
                continue;
            }
            for (dc, field) in line.split('\t').enumerate() {
                let at = CellRef::new(row + dr as u32, col + dc as u32);
                ops.push(build_set_op(session, sheet, at, field));
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

fn apply_style_range(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    edit: impl Fn(&mut Style),
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut ops = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                let at = CellRef::new(r, c);
                let mut style = session
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|sh| sh.cells.get(at))
                    .and_then(|cell| cell.style)
                    .and_then(|id| session.workbook().styles.get(id))
                    .cloned()
                    .unwrap_or_default();
                edit(&mut style);
                let style_id = if style.is_default() {
                    None
                } else {
                    Some(session.workbook_mut().intern_style(style))
                };
                ops.push(EditOperation::SetStyle {
                    sheet,
                    at,
                    style: style_id,
                });
            }
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Undo the last edit.
#[wasm_bindgen]
pub fn session_undo() -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.undo().map_err(js)
    })
}

/// Redo the last undone edit.
#[wasm_bindgen]
pub fn session_redo() -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.redo().map_err(js)
    })
}

/// Whether an edit can be undone.
#[wasm_bindgen]
pub fn session_can_undo() -> bool {
    with_session(|s| s.can_undo()).unwrap_or(false)
}

/// Whether an undone edit can be redone.
#[wasm_bindgen]
pub fn session_can_redo() -> bool {
    with_session(|s| s.can_redo()).unwrap_or(false)
}

/// Save the session workbook to `.xlsx` bytes.
#[wasm_bindgen]
pub fn session_save() -> Result<Vec<u8>, JsError> {
    SESSION.with(|cell| {
        let guard = cell.borrow();
        let session = guard.as_ref().ok_or_else(|| JsError::new("no session"))?;
        session.save().map_err(js)
    })
}

// ---------------------------------------------------------------------------
// Internals.
// ---------------------------------------------------------------------------

fn set_session(session: WorkbookSession) {
    SESSION.with(|cell| *cell.borrow_mut() = Some(session));
}

fn with_session<R>(f: impl FnOnce(&WorkbookSession) -> R) -> Option<R> {
    SESSION.with(|cell| cell.borrow().as_ref().map(f))
}

fn build_set_op(
    session: &mut WorkbookSession,
    sheet: usize,
    at: CellRef,
    input: &str,
) -> EditOperation {
    let trimmed = input.trim();
    let existing_style = session
        .workbook()
        .sheets
        .get(sheet)
        .and_then(|s| s.cells.get(at))
        .and_then(|c| c.style);

    if trimmed.is_empty() {
        return EditOperation::ClearCell { sheet, at };
    }

    if let Some(body) = trimmed.strip_prefix('=')
        && let Ok(expr) = parse(body)
    {
        let handle = session.workbook_mut().store_formula(expr);
        let mut cell = Cell::value(CellValue::Empty);
        cell.style = existing_style;
        cell.formula = Some(handle);
        return EditOperation::SetCell {
            sheet,
            at,
            cell: Some(cell),
        };
    }

    let value = if let Ok(n) = trimmed.parse::<f64>() {
        CellValue::Number(n)
    } else {
        CellValue::InlineString(session.workbook_mut().intern_string(trimmed))
    };
    EditOperation::SetValue { sheet, at, value }
}

fn value_text(workbook: &Workbook, value: &CellValue) -> String {
    match value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => format!("{n}"),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::Error(e) => e.to_string(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            workbook.strings.get(*id).unwrap_or_default().to_owned()
        }
    }
}

fn viewport_px(width_px: u32, height_px: u32, dpi: u32) -> Viewport {
    Viewport {
        x: 0,
        y: 0,
        width: px_to_twips(width_px, dpi),
        height: px_to_twips(height_px, dpi),
    }
}

fn px_to_twips(px: u32, dpi: u32) -> i64 {
    if dpi == 0 {
        return 0;
    }
    px as i64 * 1440 / dpi as i64
}

fn js<E: std::fmt::Display>(err: E) -> JsError {
    JsError::new(&err.to_string())
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
