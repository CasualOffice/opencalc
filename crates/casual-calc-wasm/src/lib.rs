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
use std::cmp::Ordering;

use casual_calc_eval::recalculate;
use casual_calc_formula::{CellReference, Expr, parse, shift_references};
use casual_calc_import::import_package;
use casual_calc_layout::{
    DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, display_text, layout_viewport,
};
use casual_calc_model::{
    BorderEdge, Borders, Cell, CellRange, CellRef, CellValue, HAlign, Id, Sheet, SheetId, Style,
    StyleId, VAlign, Workbook,
};
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

/// Open delimited text (CSV/TSV/PSV) into the editor session. `delimiter` is the
/// separator byte (e.g. `,`, tab, `|`).
#[wasm_bindgen]
pub fn session_open_delimited(bytes: &[u8], delimiter: u8) -> Result<(), JsError> {
    let workbook = casual_calc_io::read_delimited(bytes, delimiter).map_err(js)?;
    set_session(WorkbookSession::from_workbook(workbook));
    Ok(())
}

/// Serialize a sheet to delimited text (CSV/TSV/PSV) using the cached values.
#[wasm_bindgen]
pub fn session_save_delimited(sheet: usize, delimiter: u8) -> String {
    with_session(|s| casual_calc_io::write_delimited(s.workbook(), sheet, delimiter))
        .unwrap_or_default()
}

/// Append a new blank sheet, returning its index.
#[wasm_bindgen]
pub fn session_add_sheet() -> Result<usize, JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let n = {
            let wb = session.workbook_mut();
            let n = wb.sheets.len();
            let id = SheetId(Id::from_parts(0x5348, 1000 + n as u64));
            wb.sheets.push(Sheet::new(id, format!("Sheet{}", n + 1)));
            n
        };
        // A new sheet name can resolve a previously-#REF cross-sheet reference.
        session.recalculate();
        Ok(n)
    })
}

/// Rename a sheet (names must be unique and non-empty).
#[wasm_bindgen]
pub fn session_rename_sheet(index: usize, name: &str) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(JsError::new("sheet name cannot be empty"));
        }
        let wb = session.workbook_mut();
        if wb
            .sheets
            .iter()
            .enumerate()
            .any(|(i, sh)| i != index && sh.name == name)
        {
            return Err(JsError::new("a sheet with that name already exists"));
        }
        if let Some(sh) = wb.sheets.get_mut(index) {
            sh.name = name.to_owned();
        }
        // References resolve sheets by name; recompute so cross-sheet formulas
        // pick up (or lose) the renamed target.
        session.recalculate();
        Ok(())
    })
}

/// Delete a sheet (never the last remaining one).
#[wasm_bindgen]
pub fn session_delete_sheet(index: usize) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let wb = session.workbook_mut();
        if wb.sheets.len() <= 1 {
            return Err(JsError::new("cannot delete the last sheet"));
        }
        if index < wb.sheets.len() {
            wb.sheets.remove(index);
        }
        // A cross-sheet reference onto the deleted sheet must become #REF!.
        session.recalculate();
        Ok(())
    })
}

/// Move a sheet from index `from` to index `to` (tab reorder).
#[wasm_bindgen]
pub fn session_move_sheet(from: usize, to: usize) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let sheets = &mut session.workbook_mut().sheets;
        if from >= sheets.len() || to >= sheets.len() || from == to {
            return Ok(());
        }
        let sheet = sheets.remove(from);
        sheets.insert(to, sheet);
        Ok(())
    })
}

/// Duplicate a sheet (inserted right after the source), returning its index.
#[wasm_bindgen]
pub fn session_duplicate_sheet(index: usize) -> Result<usize, JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let wb = session.workbook_mut();
        let Some(src) = wb.sheets.get(index) else {
            return Err(JsError::new("no such sheet"));
        };
        let mut clone = src.clone();
        clone.id = SheetId(Id::from_parts(0x5348, 2000 + wb.sheets.len() as u64));
        let base = src.name.clone();
        let mut n = 2;
        let mut name = format!("{base} ({n})");
        while wb.sheets.iter().any(|sh| sh.name == name) {
            n += 1;
            name = format!("{base} ({n})");
        }
        clone.name = name;
        let at = index + 1;
        wb.sheets.insert(at, clone);
        // The new sheet name may resolve references elsewhere; recompute.
        session.recalculate();
        Ok(at)
    })
}

/// Case-insensitive substring replace (used when Find & Replace isn't
/// match-case). Replaces every occurrence, emitting the replacement verbatim.
fn ci_replace(haystack: &str, needle: &str, repl: &str) -> String {
    if needle.is_empty() {
        return haystack.to_owned();
    }
    let (hay_l, needle_l) = (haystack.to_lowercase(), needle.to_lowercase());
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if hay_l[i..].starts_with(&needle_l) {
            out.push_str(repl);
            i += needle.len();
        } else {
            // advance one char (UTF-8 safe)
            let ch = haystack[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn contains_ci(hay: &str, needle: &str, match_case: bool) -> bool {
    if match_case {
        hay.contains(needle)
    } else {
        hay.to_lowercase().contains(&needle.to_lowercase())
    }
}

/// All cells whose display text contains `query`, as JSON `[{r,c}, …]`.
#[wasm_bindgen]
pub fn session_find(sheet: usize, query: &str, match_case: bool) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return "[]".to_owned();
        };
        if query.is_empty() {
            return "[]".to_owned();
        }
        let mut hits = Vec::new();
        for (at, cell) in sh.cells.iter() {
            if contains_ci(&display_text(wb, cell), query, match_case) {
                hits.push(format!("{{\"r\":{},\"c\":{}}}", at.row, at.col));
            }
        }
        format!("[{}]", hits.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Replace `find` with `replace` in every text cell, returning the count
/// (one undo step). Only string cells are touched; formulas/numbers are left.
#[wasm_bindgen]
pub fn session_replace_all(
    sheet: usize,
    find: &str,
    replace: &str,
    match_case: bool,
) -> Result<usize, JsError> {
    if find.is_empty() {
        return Ok(0);
    }
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut edits: Vec<(CellRef, String)> = Vec::new();
        if let Some(sh) = session.workbook().sheets.get(sheet) {
            for (at, c) in sh.cells.iter() {
                let text = match c.value {
                    CellValue::SharedString(id) | CellValue::InlineString(id) => {
                        session.workbook().strings.get(id).unwrap_or_default()
                    }
                    _ => continue,
                };
                if !contains_ci(text, find, match_case) {
                    continue;
                }
                let replaced = if match_case {
                    text.replace(find, replace)
                } else {
                    ci_replace(text, find, replace)
                };
                edits.push((at, replaced));
            }
        }
        let count = edits.len();
        let mut ops = Vec::with_capacity(count);
        for (at, text) in edits {
            let id = session.workbook_mut().intern_string(&text);
            ops.push(EditOperation::SetValue {
                sheet,
                at,
                value: CellValue::SharedString(id),
            });
        }
        if !ops.is_empty() {
            session.edit(EditOperation::Batch(ops)).map_err(js)?;
        }
        Ok(count)
    })
}

/// The data-edge cell reached by Ctrl+Arrow from `(row,col)` moving by
/// `(dr,dc)` ∈ {-1,0,1}, using Excel's block-jump rule. Returns JSON `{row,col}`.
#[wasm_bindgen]
pub fn session_edge(sheet: usize, row: u32, col: u32, dr: i32, dc: i32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return format!("{{\"row\":{row},\"col\":{col}}}");
        };
        let (mut max_r, mut max_c) = (0u32, 0u32);
        for (at, _) in sh.cells.iter() {
            max_r = max_r.max(at.row);
            max_c = max_c.max(at.col);
        }
        let occ = |r: i64, c: i64| -> bool {
            r >= 0
                && c >= 0
                && r <= max_r as i64
                && c <= max_c as i64
                && sh
                    .cells
                    .get(CellRef::new(r as u32, c as u32))
                    .is_some_and(|cell| !cell.value.is_empty() || cell.formula.is_some())
        };
        let in_range = |r: i64, c: i64| r >= 0 && c >= 0 && r <= max_r as i64 && c <= max_c as i64;
        let (dr, dc) = (dr as i64, dc as i64);
        let (mut r, mut c) = (row as i64, col as i64);
        if in_range(r + dr, c + dc) {
            if occ(r, c) && occ(r + dr, c + dc) {
                // In a filled run: stop at the last filled cell before a gap.
                while in_range(r + dr, c + dc) && occ(r + dr, c + dc) {
                    r += dr;
                    c += dc;
                }
            } else {
                // Skip blanks to the next filled cell (or the used edge).
                r += dr;
                c += dc;
                while in_range(r + dr, c + dc) && !occ(r, c) {
                    r += dr;
                    c += dc;
                }
            }
        }
        format!("{{\"row\":{},\"col\":{}}}", r.max(0), c.max(0))
    })
    .unwrap_or_else(|| format!("{{\"row\":{row},\"col\":{col}}}"))
}

/// The frozen-pane counts of a sheet as JSON `{ rows, cols }`.
#[wasm_bindgen]
pub fn session_frozen(sheet: usize) -> String {
    with_session(|s| {
        let v = s.workbook().sheets.get(sheet).map(|sh| sh.view);
        let (r, c) = v.map_or((0, 0), |v| (v.frozen_rows, v.frozen_cols));
        format!("{{\"rows\":{r},\"cols\":{c}}}")
    })
    .unwrap_or_else(|| "{\"rows\":0,\"cols\":0}".to_owned())
}

/// Set the number of frozen rows/columns on a sheet.
#[wasm_bindgen]
pub fn session_set_freeze(sheet: usize, rows: u32, cols: u32) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            sh.view.frozen_rows = rows;
            sh.view.frozen_cols = cols;
        }
        Ok(())
    })
}

/// A sheet's tab color as an `RRGGBB` hex string, or `""` if uncolored.
#[wasm_bindgen]
pub fn session_tab_color(sheet: usize) -> String {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.tab_color.clone())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// Set (or, with an empty/invalid string, clear) a sheet's tab color. Accepts
/// `RRGGBB` or `#RRGGBB`; stored uppercased without the `#`.
#[wasm_bindgen]
pub fn session_set_tab_color(sheet: usize, hex: &str) -> Result<(), JsError> {
    let cleaned = hex.trim().trim_start_matches('#');
    let color = if cleaned.len() == 6 && cleaned.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(cleaned.to_ascii_uppercase())
    } else {
        None
    };
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            sh.tab_color = color;
        }
        Ok(())
    })
}

/// The merged ranges of a sheet as JSON `[{r0,c0,r1,c1}, …]`.
#[wasm_bindgen]
pub fn session_merges(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .merges
            .iter()
            .map(|m| {
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}}}",
                    m.start.row, m.start.col, m.end.row, m.end.col
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Whether a merge intersects the box `[r0,c0]..[r1,c1]`.
fn merge_hits(m: &casual_calc_model::CellRange, r0: u32, c0: u32, r1: u32, c1: u32) -> bool {
    !(m.end.row < r0 || m.start.row > r1 || m.end.col < c0 || m.start.col > c1)
}

/// Merge a range into one cell (drops any merges it overlaps).
#[wasm_bindgen]
pub fn session_merge_cells(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            sh.merges.retain(|m| !merge_hits(m, r0, c0, r1, c1));
            if r0 != r1 || c0 != c1 {
                sh.merges
                    .push(CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1)));
            }
        }
        Ok(())
    })
}

/// Remove any merges intersecting a range.
#[wasm_bindgen]
pub fn session_unmerge_cells(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            sh.merges.retain(|m| !merge_hits(m, r0, c0, r1, c1));
        }
        Ok(())
    })
}

/// Aggregate stats over a range for the status bar, as JSON
/// `{ count, numeric, sum, avg, min, max }` (count = non-empty cells; the rest
/// cover numeric cells only; `avg/min/max` are null when there are none).
#[wasm_bindgen]
pub fn session_range_stats(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return "{\"count\":0,\"numeric\":0}".to_owned();
        };
        let (mut count, mut numeric, mut sum) = (0u64, 0u64, 0f64);
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for r in r0..=r1 {
            for c in c0..=c1 {
                let Some(cell) = sh.cells.get(CellRef::new(r, c)) else {
                    continue;
                };
                if cell.value.is_empty() {
                    continue;
                }
                count += 1;
                if let CellValue::Number(n) = cell.value {
                    numeric += 1;
                    sum += n;
                    min = min.min(n);
                    max = max.max(n);
                }
            }
        }
        if numeric == 0 {
            return format!("{{\"count\":{count},\"numeric\":0}}");
        }
        let avg = sum / numeric as f64;
        format!(
            "{{\"count\":{count},\"numeric\":{numeric},\"sum\":{sum},\"avg\":{avg},\"min\":{min},\"max\":{max}}}"
        )
    })
    .unwrap_or_else(|| "{\"count\":0,\"numeric\":0}".to_owned())
}

fn commit_edit(op: EditOperation) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.edit(op).map_err(js)
    })
}

/// Insert `count` blank rows before `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_insert_rows(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    commit_edit(EditOperation::InsertRows { sheet, at, count })
}

/// Delete `count` rows starting at `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_delete_rows(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    commit_edit(EditOperation::DeleteRows { sheet, at, count })
}

/// Insert `count` blank columns before `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_insert_columns(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    commit_edit(EditOperation::InsertColumns { sheet, at, count })
}

/// Delete `count` columns starting at `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_delete_columns(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    commit_edit(EditOperation::DeleteColumns { sheet, at, count })
}

/// Set vertical alignment across a range: `top`/`middle`/`bottom`, or empty to
/// clear (one undo step).
#[wasm_bindgen]
pub fn session_set_valign(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    valign: &str,
) -> Result<(), JsError> {
    let value = match valign {
        "top" => Some(VAlign::Top),
        "middle" | "center" => Some(VAlign::Middle),
        "bottom" => Some(VAlign::Bottom),
        _ => None,
    };
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.valign = value)
}

/// Set (or clear, with empty) the font family across a range (one undo step).
#[wasm_bindgen]
pub fn session_set_font_name(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    name: &str,
) -> Result<(), JsError> {
    let font = (!name.is_empty()).then(|| name.to_owned());
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.font_name = font.clone())
}

/// Set (or clear, with 0) the font size in points across a range (one undo step).
#[wasm_bindgen]
pub fn session_set_font_size(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    points: f64,
) -> Result<(), JsError> {
    let hp = (points > 0.0).then(|| (points * 2.0).round() as u32);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.font_size_hp = hp)
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
        let sh = s.workbook().sheets.get(sheet);
        let sizing = sh.map(|sh| if columns { &sh.columns } else { &sh.rows });
        let mut out = String::from("[");
        for i in 0..count {
            if i > 0 {
                out.push(',');
            }
            let line = first + i;
            // Hidden lines collapse to 0 px so the editor skips them.
            let hidden = sh.is_some_and(|sh| {
                if columns {
                    sh.hidden_cols.contains(&line)
                } else {
                    sh.hidden_rows.contains(&line)
                }
            });
            let twips = if hidden {
                0
            } else {
                sizing.map_or(fallback, |sz| sz.size(line, fallback))
            };
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

/// Set every column's width to `px` (the sheet default, clearing overrides).
#[wasm_bindgen]
pub fn session_set_all_col_width(sheet: usize, px: u32) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            sh.columns.default = Some(resize_px_to_twips(px));
            sh.columns.sizes.clear();
        }
        Ok(())
    })
}

/// Set every row's height to `px` (the sheet default, clearing overrides).
#[wasm_bindgen]
pub fn session_set_all_row_height(sheet: usize, px: u32) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            sh.rows.default = Some(resize_px_to_twips(px));
            sh.rows.sizes.clear();
        }
        Ok(())
    })
}

/// Set the width of columns `c0..=c1` to `px` (one undo step).
#[wasm_bindgen]
pub fn session_set_col_width_range(sheet: usize, c0: u32, c1: u32, px: u32) -> Result<(), JsError> {
    let width = Some(resize_px_to_twips(px));
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let ops = (c0..=c1)
            .map(|col| EditOperation::SetColumnWidth { sheet, col, width })
            .collect();
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Set the height of rows `r0..=r1` to `px` (one undo step).
#[wasm_bindgen]
pub fn session_set_row_height_range(
    sheet: usize,
    r0: u32,
    r1: u32,
    px: u32,
) -> Result<(), JsError> {
    let height = Some(resize_px_to_twips(px));
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let ops = (r0..=r1)
            .map(|row| EditOperation::SetRowHeight { sheet, row, height })
            .collect();
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
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
            let has_border = style.is_some_and(|s| s.border.is_some());
            if text.is_empty() && fill.is_empty() && !has_border {
                continue;
            }
            // Explicit alignment wins; otherwise numbers/bools/errors go right.
            let align = match style.and_then(|s| s.align) {
                Some(HAlign::Left) => "l",
                Some(HAlign::Center) => "c",
                Some(HAlign::Right) => "r",
                None => match cell.value {
                    CellValue::Number(_) | CellValue::Bool(_) | CellValue::Error(_) => "r",
                    _ => "l",
                },
            };
            let mut extra = String::new();
            if style.is_some_and(|s| s.bold) {
                extra.push_str(",\"b\":1");
            }
            if style.is_some_and(|s| s.italic) {
                extra.push_str(",\"i\":1");
            }
            if style.is_some_and(|s| s.underline) {
                extra.push_str(",\"u\":1");
            }
            if style.is_some_and(|s| s.wrap) {
                extra.push_str(",\"w\":1");
            }
            if style.is_some_and(|s| s.strike) {
                extra.push_str(",\"st\":1");
            }
            if let Some(fname) = style.and_then(|s| s.font_name.as_deref()) {
                extra.push_str(&format!(",\"fn\":{}", json_string(fname)));
            }
            if let Some(hp) = style.and_then(|s| s.font_size_hp) {
                extra.push_str(&format!(",\"fs\":{}", hp as f64 / 2.0));
            }
            if let Some(va) = style.and_then(|s| s.valign) {
                let t = match va {
                    VAlign::Top => "t",
                    VAlign::Middle => "m",
                    VAlign::Bottom => "b",
                };
                extra.push_str(&format!(",\"va\":\"{t}\""));
            }
            if let Some(fc) = style.and_then(|s| s.font_color.as_deref()) {
                extra.push_str(&format!(",\"fc\":{}", json_string(fc)));
            }
            if !fill.is_empty() {
                extra.push_str(&format!(",\"bg\":{}", json_string(&fill)));
            }
            if let Some(bd) = style.and_then(|s| s.border.as_ref()) {
                extra.push_str(&format!(",\"bd\":{}", border_json(bd)));
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

/// Validate a formula string (with or without the leading `=`). Returns `""`
/// when it parses, otherwise a human-readable parse error. Non-formula input
/// (no leading `=`) is always valid.
#[wasm_bindgen]
pub fn validate_formula(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.strip_prefix('=') {
        Some(body) => match parse(body) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        },
        None => String::new(),
    }
}

/// The catalog of supported worksheet functions as JSON `[{n,sig}, …]`, sorted
/// by name — drives the editor's formula autocomplete. Kept in step with the
/// dispatch table in `casual-calc-eval::functions`.
#[wasm_bindgen]
pub fn function_catalog() -> String {
    const FUNCS: &[(&str, &str)] = &[
        ("ABS", "ABS(number)"),
        ("AND", "AND(logical1, …)"),
        ("AVERAGE", "AVERAGE(number1, …)"),
        ("AVERAGEIF", "AVERAGEIF(range, criteria, [average_range])"),
        ("CEILING", "CEILING(number, significance)"),
        ("CHOOSE", "CHOOSE(index, value1, …)"),
        ("CONCAT", "CONCAT(text1, …)"),
        ("CONCATENATE", "CONCATENATE(text1, …)"),
        ("COUNT", "COUNT(value1, …)"),
        ("COUNTA", "COUNTA(value1, …)"),
        ("COUNTIF", "COUNTIF(range, criteria)"),
        ("DATE", "DATE(year, month, day)"),
        ("DAY", "DAY(serial_number)"),
        ("EDATE", "EDATE(start_date, months)"),
        ("EOMONTH", "EOMONTH(start_date, months)"),
        ("EXACT", "EXACT(text1, text2)"),
        ("FIND", "FIND(find_text, within_text, [start])"),
        ("FLOOR", "FLOOR(number, significance)"),
        ("HLOOKUP", "HLOOKUP(lookup, table, row, [exact])"),
        ("IF", "IF(logical_test, value_if_true, value_if_false)"),
        ("IFERROR", "IFERROR(value, value_if_error)"),
        ("INDEX", "INDEX(array, row_num, [col_num])"),
        ("INT", "INT(number)"),
        ("LEFT", "LEFT(text, [num_chars])"),
        ("LEN", "LEN(text)"),
        ("LOWER", "LOWER(text)"),
        ("MATCH", "MATCH(lookup, array, [match_type])"),
        ("MAX", "MAX(number1, …)"),
        ("MID", "MID(text, start_num, num_chars)"),
        ("MIN", "MIN(number1, …)"),
        ("MOD", "MOD(number, divisor)"),
        ("MONTH", "MONTH(serial_number)"),
        ("NOT", "NOT(logical)"),
        ("OR", "OR(logical1, …)"),
        ("POWER", "POWER(number, power)"),
        ("PRODUCT", "PRODUCT(number1, …)"),
        ("PROPER", "PROPER(text)"),
        ("REPLACE", "REPLACE(old, start, num_chars, new)"),
        ("REPT", "REPT(text, number_times)"),
        ("RIGHT", "RIGHT(text, [num_chars])"),
        ("ROUND", "ROUND(number, num_digits)"),
        ("ROUNDDOWN", "ROUNDDOWN(number, num_digits)"),
        ("ROUNDUP", "ROUNDUP(number, num_digits)"),
        ("SEARCH", "SEARCH(find_text, within_text, [start])"),
        ("SIGN", "SIGN(number)"),
        ("SQRT", "SQRT(number)"),
        ("SUBSTITUTE", "SUBSTITUTE(text, old, new, [instance])"),
        ("SUM", "SUM(number1, …)"),
        ("SUMIF", "SUMIF(range, criteria, [sum_range])"),
        ("TRIM", "TRIM(text)"),
        ("TRUNC", "TRUNC(number, [num_digits])"),
        ("UPPER", "UPPER(text)"),
        ("VALUE", "VALUE(text)"),
        ("VLOOKUP", "VLOOKUP(lookup, table, col, [exact])"),
        ("WEEKDAY", "WEEKDAY(serial_number, [type])"),
        ("YEAR", "YEAR(serial_number)"),
    ];
    let items: Vec<String> = FUNCS
        .iter()
        .map(|(n, sig)| format!("{{\"n\":{},\"sig\":{}}}", json_string(n), json_string(sig)))
        .collect();
    format!("[{}]", items.join(","))
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
        // Preserve the cell's other formatting (number format, italic, font
        // color, borders); only bold and fill are being set here.
        let mut style = session
            .workbook()
            .sheets
            .get(sheet)
            .and_then(|s| s.cells.get(at))
            .and_then(|c| c.style)
            .and_then(|id| session.workbook().styles.get(id))
            .cloned()
            .unwrap_or_default();
        style.bold = bold;
        style.fill_color = (!fill.is_empty()).then(|| fill.to_owned());
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

/// Whether every cell in a range satisfies `pred` on its style.
fn range_all(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    pred: impl Fn(&Style) -> bool,
) -> bool {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return false;
        };
        let mut any = false;
        for r in r0..=r1 {
            for c in c0..=c1 {
                any = true;
                let ok = sh
                    .cells
                    .get(CellRef::new(r, c))
                    .and_then(|cell| cell.style)
                    .and_then(|id| wb.styles.get(id))
                    .is_some_and(&pred);
                if !ok {
                    return false;
                }
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
    let target = !range_all(sheet, r0, c0, r1, c1, |st| st.bold);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.bold = target)
}

/// Toggle italic across a range (one undo step).
#[wasm_bindgen]
pub fn session_toggle_italic(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let target = !range_all(sheet, r0, c0, r1, c1, |st| st.italic);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.italic = target)
}

/// Toggle underline across a range (one undo step).
#[wasm_bindgen]
pub fn session_toggle_underline(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let target = !range_all(sheet, r0, c0, r1, c1, |st| st.underline);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.underline = target)
}

/// Toggle text wrapping across a range (one undo step).
#[wasm_bindgen]
pub fn session_toggle_wrap(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let target = !range_all(sheet, r0, c0, r1, c1, |st| st.wrap);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.wrap = target)
}

/// Drag-fill: fill the destination box from the source box, tiling the source
/// pattern and shifting relative formula references by each cell's offset
/// (one undo step). Cells inside the source box are left untouched.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_fill(
    sheet: usize,
    sr0: u32,
    sc0: u32,
    sr1: u32,
    sc1: u32,
    dr0: u32,
    dc0: u32,
    dr1: u32,
    dc1: u32,
) -> Result<(), JsError> {
    let (src_rows, src_cols) = ((sr1 - sr0 + 1) as i64, (sc1 - sc0 + 1) as i64);
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        // Pass 1 (immutable): resolve each destination cell's source + shifted formula.
        struct Pending {
            at: CellRef,
            value: CellValue,
            style: Option<StyleId>,
            formula: Option<Expr>,
        }
        let mut pending: Vec<Pending> = Vec::new();
        if let Some(sh) = session.workbook().sheets.get(sheet) {
            let wb = session.workbook();
            // A numeric literal (no formula) at (r,c), for series detection.
            let num_lit = |r: u32, c: u32| -> Option<f64> {
                sh.cells
                    .get(CellRef::new(r, c))
                    .and_then(|cell| match cell.value {
                        CellValue::Number(n) if cell.formula.is_none() => Some(n),
                        _ => None,
                    })
            };
            // If the fill grows along exactly one axis and each line of the
            // source is a numeric arithmetic sequence (>=2 cells, constant
            // step), extend the sequence instead of tiling — Excel's autofill.
            let vertical = dc0 == sc0 && dc1 == sc1 && (dr1 > sr1 || dr0 < sr0);
            let horizontal = dr0 == sr0 && dr1 == sr1 && (dc1 > sc1 || dc0 < sc0);
            let arithmetic = |vals: &[Option<f64>]| -> Option<(f64, f64)> {
                if vals.len() < 2 || vals.iter().any(|v| v.is_none()) {
                    return None;
                }
                let step = vals[1].unwrap() - vals[0].unwrap();
                for w in vals.windows(2) {
                    if (w[1].unwrap() - w[0].unwrap() - step).abs() > 1e-9 {
                        return None;
                    }
                }
                Some((vals[0].unwrap(), step))
            };
            // Per-line (v0, step): by column for a vertical fill, by row for a
            // horizontal one.
            let col_series: Vec<Option<(f64, f64)>> = if vertical {
                (sc0..=sc1)
                    .map(|c| arithmetic(&(sr0..=sr1).map(|r| num_lit(r, c)).collect::<Vec<_>>()))
                    .collect()
            } else {
                Vec::new()
            };
            let row_series: Vec<Option<(f64, f64)>> = if horizontal {
                (sr0..=sr1)
                    .map(|r| arithmetic(&(sc0..=sc1).map(|c| num_lit(r, c)).collect::<Vec<_>>()))
                    .collect()
            } else {
                Vec::new()
            };

            for dr in dr0..=dr1 {
                for dc in dc0..=dc1 {
                    if dr >= sr0 && dr <= sr1 && dc >= sc0 && dc <= sc1 {
                        continue; // don't overwrite the source
                    }
                    let sr = sr0 as i64 + (dr as i64 - sr0 as i64).rem_euclid(src_rows);
                    let sc = sc0 as i64 + (dc as i64 - sc0 as i64).rem_euclid(src_cols);
                    let at = CellRef::new(dr, dc);
                    // Series value along the fill axis, if one was detected.
                    let series_value = if vertical {
                        col_series[(dc - sc0) as usize]
                            .map(|(v0, step)| v0 + step * (dr as i64 - sr0 as i64) as f64)
                    } else if horizontal {
                        row_series[(dr - sr0) as usize]
                            .map(|(v0, step)| v0 + step * (dc as i64 - sc0 as i64) as f64)
                    } else {
                        None
                    };
                    if let Some(v) = series_value {
                        // Numeric series: extend the value, tile the source style.
                        let style = sh
                            .cells
                            .get(CellRef::new(sr as u32, sc as u32))
                            .and_then(|c| c.style);
                        pending.push(Pending {
                            at,
                            value: CellValue::Number(v),
                            style,
                            formula: None,
                        });
                        continue;
                    }
                    match sh.cells.get(CellRef::new(sr as u32, sc as u32)) {
                        Some(c) => {
                            let formula = c
                                .formula
                                .and_then(|h| wb.formula(h))
                                .map(|e| shift_references(e, dr as i64 - sr, dc as i64 - sc));
                            pending.push(Pending {
                                at,
                                value: c.value.clone(),
                                style: c.style,
                                formula,
                            });
                        }
                        None => pending.push(Pending {
                            at,
                            value: CellValue::Empty,
                            style: None,
                            formula: None,
                        }),
                    }
                }
            }
        }
        // Pass 2 (mutable): store shifted formulas and build the edit batch.
        let mut ops = Vec::with_capacity(pending.len());
        for p in pending {
            let cell = if p.value.is_empty() && p.style.is_none() && p.formula.is_none() {
                None
            } else {
                let mut c = Cell::value(p.value);
                c.style = p.style;
                if let Some(expr) = p.formula {
                    c.formula = Some(session.workbook_mut().store_formula(expr));
                }
                Some(c)
            };
            ops.push(EditOperation::SetCell {
                sheet,
                at: p.at,
                cell,
            });
        }
        if ops.is_empty() {
            return Ok(());
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Sort the rows of a range `[r0..=r1] × [c0..=c1]` by the values in column
/// `key_col`, moving each whole row (values + styles + formula handles) as a
/// unit — one undo step. Blanks sort last in both directions; otherwise numbers
/// order before text, text case-insensitively. Formula handles move verbatim
/// (their references are not re-anchored — sorting a data range is the intent).
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_sort_range(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    key_col: u32,
    ascending: bool,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;

        // Pass 1 (immutable): snapshot each row's cells, its owned sort key, and
        // each formula cell's resolved AST (so we can re-anchor it on the move).
        struct RowCell {
            cell: Cell,
            formula: Option<Expr>,
        }
        struct Row {
            src_row: u32,
            key: (u8, f64, String),
            blank: bool,
            cells: Vec<Option<RowCell>>,
        }
        let mut rows: Vec<Row> = Vec::new();
        if let Some(sh) = session.workbook().sheets.get(sheet) {
            let wb = session.workbook();
            for r in r0..=r1 {
                let kv = sh
                    .cells
                    .get(CellRef::new(r, key_col))
                    .map(|c| c.value.clone())
                    .unwrap_or(CellValue::Empty);
                let key = match &kv {
                    CellValue::Number(n) => (0u8, *n, String::new()),
                    CellValue::Bool(b) => (0, if *b { 1.0 } else { 0.0 }, String::new()),
                    CellValue::SharedString(id) | CellValue::InlineString(id) => (
                        1,
                        0.0,
                        wb.strings.get(*id).unwrap_or_default().to_lowercase(),
                    ),
                    CellValue::Error(_) => (2, 0.0, String::new()),
                    CellValue::Empty => (3, 0.0, String::new()),
                };
                let cells = (c0..=c1)
                    .map(|c| {
                        sh.cells.get(CellRef::new(r, c)).map(|cell| RowCell {
                            cell: cell.clone(),
                            formula: cell.formula.and_then(|h| wb.formula(h)).cloned(),
                        })
                    })
                    .collect();
                rows.push(Row {
                    src_row: r,
                    key,
                    blank: kv.is_empty(),
                    cells,
                });
            }
        } else {
            return Ok(());
        }

        // Keep blanks pinned to the end (Excel behavior), sort the rest by key.
        let (mut filled, empties): (Vec<Row>, Vec<Row>) = rows.into_iter().partition(|r| !r.blank);
        filled.sort_by(|a, b| {
            let ord = a
                .key
                .0
                .cmp(&b.key.0)
                .then_with(|| a.key.1.partial_cmp(&b.key.1).unwrap_or(Ordering::Equal))
                .then_with(|| a.key.2.cmp(&b.key.2));
            if ascending { ord } else { ord.reverse() }
        });
        filled.extend(empties);

        // Pass 2 (mutable): write each row back to its sorted position. A
        // per-row formula's references are re-anchored by the row delta ONLY
        // when they point at a cell that moves with this row — i.e. a same-row,
        // relative, same-sheet reference inside the sorted columns (e.g. =B2*C2
        // -> =B5*C5). References outside the block (a header, a constant one row
        // up, another sheet) are pinned, exactly as Excel keeps them.
        let mut ops = Vec::with_capacity(filled.len() * (c1 - c0 + 1) as usize);
        for (i, row) in filled.into_iter().enumerate() {
            let r = r0 + i as u32;
            let dr = r as i64 - row.src_row as i64;
            for (j, c) in (c0..=c1).enumerate() {
                let cell = row.cells[j].as_ref().map(|rc| {
                    let mut out = rc.cell.clone();
                    if let Some(expr) = &rc.formula {
                        let shifted = sort_reanchor(expr, dr, row.src_row, c0, c1);
                        out.formula = Some(session.workbook_mut().store_formula(shifted));
                    }
                    out
                });
                ops.push(EditOperation::SetCell {
                    sheet,
                    at: CellRef::new(r, c),
                    cell,
                });
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Whether a reference moves with a sorted row: relative, unqualified, on the
/// source row, and inside the sorted columns.
fn ref_moves_with_row(r: &CellReference, src_row: u32, c0: u32, c1: u32) -> bool {
    !r.row_absolute && r.sheet.is_none() && r.row == src_row && r.col >= c0 && r.col <= c1
}

fn shifted_row(r: &CellReference, dr: i64) -> CellReference {
    let mut out = r.clone();
    out.row = (r.row as i64 + dr).max(0) as u32;
    out
}

/// Re-anchor a formula for a row moved by `dr` during a sort: shift only the
/// references that travel with the row (see [`ref_moves_with_row`]); a range is
/// shifted only when both endpoints do, so a multi-row range is never split.
fn sort_reanchor(expr: &Expr, dr: i64, src_row: u32, c0: u32, c1: u32) -> Expr {
    match expr {
        Expr::Reference(r) if ref_moves_with_row(r, src_row, c0, c1) => {
            Expr::Reference(shifted_row(r, dr))
        }
        Expr::Range(a, b)
            if ref_moves_with_row(a, src_row, c0, c1) && ref_moves_with_row(b, src_row, c0, c1) =>
        {
            Expr::Range(shifted_row(a, dr), shifted_row(b, dr))
        }
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(sort_reanchor(operand, dr, src_row, c0, c1)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(sort_reanchor(left, dr, src_row, c0, c1)),
            right: Box::new(sort_reanchor(right, dr, src_row, c0, c1)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| sort_reanchor(a, dr, src_row, c0, c1))
                .collect(),
        },
        other => other.clone(),
    }
}

/// Toggle strikethrough across a range (one undo step).
#[wasm_bindgen]
pub fn session_toggle_strike(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let target = !range_all(sheet, r0, c0, r1, c1, |st| st.strike);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.strike = target)
}

/// Hide rows `r0..=r1` on a sheet.
#[wasm_bindgen]
pub fn session_hide_rows(sheet: usize, r0: u32, r1: u32) -> Result<(), JsError> {
    hidden_edit(sheet, r0, r1, false, true)
}
/// Hide columns `c0..=c1` on a sheet.
#[wasm_bindgen]
pub fn session_hide_cols(sheet: usize, c0: u32, c1: u32) -> Result<(), JsError> {
    hidden_edit(sheet, c0, c1, true, true)
}
/// Unhide any hidden rows in `r0..=r1`.
#[wasm_bindgen]
pub fn session_unhide_rows(sheet: usize, r0: u32, r1: u32) -> Result<(), JsError> {
    hidden_edit(sheet, r0, r1, false, false)
}
/// Unhide any hidden columns in `c0..=c1`.
#[wasm_bindgen]
pub fn session_unhide_cols(sheet: usize, c0: u32, c1: u32) -> Result<(), JsError> {
    hidden_edit(sheet, c0, c1, true, false)
}

fn hidden_edit(sheet: usize, a: u32, b: u32, columns: bool, hide: bool) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if let Some(sh) = session.workbook_mut().sheets.get_mut(sheet) {
            let set = if columns {
                &mut sh.hidden_cols
            } else {
                &mut sh.hidden_rows
            };
            for i in a..=b {
                if hide {
                    set.insert(i);
                } else {
                    set.remove(&i);
                }
            }
        }
        Ok(())
    })
}

/// Set (or clear, with empty hex) the font color across a range (one undo step).
#[wasm_bindgen]
pub fn session_set_font_color(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    hex: &str,
) -> Result<(), JsError> {
    let color = (!hex.is_empty()).then(|| hex.to_owned());
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.font_color = color.clone()
    })
}

/// Set horizontal alignment across a range: `left`/`center`/`right`, or empty to
/// clear (one undo step).
#[wasm_bindgen]
pub fn session_set_align(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    align: &str,
) -> Result<(), JsError> {
    let value = HAlign::from_ooxml(align);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.align = value)
}

/// Set (or clear, with empty code) the number format across a range (one undo
/// step). Codes are OOXML format strings, e.g. `0.00`, `0%`, `$#,##0.00`.
#[wasm_bindgen]
pub fn session_set_number_format(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    code: &str,
) -> Result<(), JsError> {
    let format = (!code.is_empty()).then(|| code.to_owned());
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.number_format = format.clone()
    })
}

/// The active cell's formatting as JSON (drives the toolbar's active states):
/// `{ b, i, u, al, nf, fc, bg }` — flags present only when set.
#[wasm_bindgen]
pub fn session_cell_format(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let style = wb
            .sheets
            .get(sheet)
            .and_then(|sh| sh.cells.get(CellRef::new(row, col)))
            .and_then(|cell| cell.style)
            .and_then(|id| wb.styles.get(id));
        let Some(st) = style else {
            return "{}".to_owned();
        };
        let mut parts: Vec<String> = Vec::new();
        if st.bold {
            parts.push("\"b\":1".to_owned());
        }
        if st.italic {
            parts.push("\"i\":1".to_owned());
        }
        if st.underline {
            parts.push("\"u\":1".to_owned());
        }
        if st.strike {
            parts.push("\"st\":1".to_owned());
        }
        if st.wrap {
            parts.push("\"w\":1".to_owned());
        }
        if let Some(fname) = &st.font_name {
            parts.push(format!("\"fn\":{}", json_string(fname)));
        }
        if let Some(hp) = st.font_size_hp {
            parts.push(format!("\"fs\":{}", hp as f64 / 2.0));
        }
        if let Some(al) = st.align {
            parts.push(format!("\"al\":\"{}\"", al.ooxml()));
        }
        if let Some(va) = st.valign {
            let t = match va {
                VAlign::Top => "t",
                VAlign::Middle => "m",
                VAlign::Bottom => "b",
            };
            parts.push(format!("\"va\":\"{t}\""));
        }
        if let Some(nf) = &st.number_format {
            parts.push(format!("\"nf\":{}", json_string(nf)));
        }
        if let Some(fc) = &st.font_color {
            parts.push(format!("\"fc\":{}", json_string(fc)));
        }
        if let Some(bg) = &st.fill_color {
            parts.push(format!("\"bg\":{}", json_string(bg)));
        }
        format!("{{{}}}", parts.join(","))
    })
    .unwrap_or_else(|| "{}".to_owned())
}

/// Whether every cell in a range carries a full (four-edge) border.
#[wasm_bindgen]
pub fn session_range_bordered(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> bool {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return false;
        };
        let mut any = false;
        for r in r0..=r1 {
            for c in c0..=c1 {
                any = true;
                let full = sh
                    .cells
                    .get(CellRef::new(r, c))
                    .and_then(|cell| cell.style)
                    .and_then(|id| wb.styles.get(id))
                    .and_then(|st| st.border.as_ref())
                    .is_some_and(|b| {
                        b.left.is_some()
                            && b.right.is_some()
                            && b.top.is_some()
                            && b.bottom.is_some()
                    });
                if !full {
                    return false;
                }
            }
        }
        any
    })
    .unwrap_or(false)
}

/// Toggle a full thin box border across a range (one undo step).
#[wasm_bindgen]
pub fn session_toggle_border(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let on = !session_range_bordered(sheet, r0, c0, r1, c1);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.border = on.then(full_thin_border);
    })
}

/// Apply a border preset across a range (one undo step). `kind` is one of
/// `all`, `outer`, or `none`.
#[wasm_bindgen]
pub fn session_set_border(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
) -> Result<(), JsError> {
    let kind = kind.to_owned();
    apply_style_range_pos(sheet, r0, c0, r1, c1, move |r, c, st| {
        st.border = match kind.as_str() {
            "none" => None,
            "all" => Some(full_thin_border()),
            "outer" => {
                let edge = || {
                    Some(BorderEdge {
                        style: "thin".to_owned(),
                        color: None,
                    })
                };
                let b = Borders {
                    top: (r == r0).then(edge).flatten(),
                    bottom: (r == r1).then(edge).flatten(),
                    left: (c == c0).then(edge).flatten(),
                    right: (c == c1).then(edge).flatten(),
                };
                (!b.is_empty()).then_some(b)
            }
            _ => st.border.clone(),
        };
    })
}

/// A four-edge thin border with the default (auto) color.
fn full_thin_border() -> Borders {
    let edge = || {
        Some(BorderEdge {
            style: "thin".to_owned(),
            color: None,
        })
    };
    Borders {
        left: edge(),
        right: edge(),
        top: edge(),
        bottom: edge(),
    }
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

/// Clear the *contents* of a range (value + formula) while keeping each cell's
/// style — what the Delete key does in every spreadsheet. A cell with no style
/// is removed entirely; a styled cell is reset to an empty value so its fill,
/// borders and number format survive.
#[wasm_bindgen]
pub fn session_clear_contents(
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
                let at = CellRef::new(r, c);
                let style = session
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|s| s.cells.get(at))
                    .and_then(|cl| cl.style);
                match style {
                    Some(sid) => {
                        let mut cleared = Cell::value(CellValue::Empty);
                        cleared.style = Some(sid);
                        ops.push(EditOperation::SetCell {
                            sheet,
                            at,
                            cell: Some(cleared),
                        });
                    }
                    None => ops.push(EditOperation::ClearCell { sheet, at }),
                }
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

/// A cell captured on the internal clipboard, relative to the copy origin.
struct ClipCell {
    dr: u32,
    dc: u32,
    cell: Cell,
    formula: Option<Expr>,
}
/// The internal (rich) clipboard: keeps values, styles, and resolved formula
/// ASTs so a paste can reproduce formulas (reference-shifted) and formatting —
/// unlike the text-only OS clipboard.
struct Clip {
    sheet: usize,
    r0: u32,
    c0: u32,
    cut: bool,
    cells: Vec<ClipCell>,
}
thread_local! {
    static CLIP: RefCell<Option<Clip>> = const { RefCell::new(None) };
}

/// Snapshot a range onto the internal clipboard (value + style + formula AST).
/// `cut` marks the source to be cleared on the next paste. The OS clipboard TSV
/// is produced separately by [`session_copy_tsv`].
#[wasm_bindgen]
pub fn session_clip_copy(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32, cut: bool) {
    let _ = with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return;
        };
        let mut cells = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if let Some(cell) = sh.cells.get(CellRef::new(r, c)) {
                    let formula = cell.formula.and_then(|h| wb.formula(h)).cloned();
                    cells.push(ClipCell {
                        dr: r - r0,
                        dc: c - c0,
                        cell: cell.clone(),
                        formula,
                    });
                }
            }
        }
        CLIP.with(|cl| {
            *cl.borrow_mut() = Some(Clip {
                sheet,
                r0,
                c0,
                cut,
                cells,
            })
        });
    });
}

/// Whether the internal clipboard currently holds a snapshot.
#[wasm_bindgen]
pub fn session_clip_has() -> bool {
    CLIP.with(|cl| cl.borrow().is_some())
}

/// Paste the internal clipboard with its top-left at `(row, col)`: formulas are
/// reference-shifted by the paste delta (absolute `$` anchors held), styles are
/// reproduced, and — for a cut — the source cells are cleared in the same undo
/// step. The clipboard is consumed on a cut, retained on a copy.
#[wasm_bindgen]
pub fn session_clip_paste(sheet: usize, row: u32, col: u32) -> Result<(), JsError> {
    session_clip_paste_mode(sheet, row, col, "all")
}

/// Paste-special: `mode` selects what is reproduced —
/// `"all"` (value + formula + style, and honors a cut),
/// `"values"` (the cached value only, keeping the target's formatting),
/// `"formats"` (the source style only, keeping the target's value).
/// A cut only takes effect for `"all"` (Excel disables cut with paste-special).
#[wasm_bindgen]
pub fn session_clip_paste_mode(
    sheet: usize,
    row: u32,
    col: u32,
    mode: &str,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let (ops, was_cut, empty) = CLIP.with(|cl| {
            let borrow = cl.borrow();
            let Some(clip) = borrow.as_ref() else {
                return (Vec::new(), false, true);
            };
            let dr = row as i64 - clip.r0 as i64;
            let dc = col as i64 - clip.c0 as i64;
            let cut = clip.cut && mode == "all";
            let mut ops = Vec::new();
            if cut {
                for cc in &clip.cells {
                    ops.push(EditOperation::ClearCell {
                        sheet: clip.sheet,
                        at: CellRef::new(clip.r0 + cc.dr, clip.c0 + cc.dc),
                    });
                }
            }
            for cc in &clip.cells {
                let at = CellRef::new(row + cc.dr, col + cc.dc);
                match mode {
                    "values" => ops.push(EditOperation::SetValue {
                        sheet,
                        at,
                        value: cc.cell.value.clone(),
                    }),
                    "formats" => ops.push(EditOperation::SetStyle {
                        sheet,
                        at,
                        style: cc.cell.style,
                    }),
                    _ => {
                        let mut out = cc.cell.clone();
                        if let Some(expr) = &cc.formula {
                            let shifted = shift_references(expr, dr, dc);
                            out.formula = Some(session.workbook_mut().store_formula(shifted));
                        }
                        ops.push(EditOperation::SetCell {
                            sheet,
                            at,
                            cell: Some(out),
                        });
                    }
                }
            }
            (ops, cut, false)
        });
        if empty {
            return Ok(());
        }
        if was_cut {
            CLIP.with(|cl| *cl.borrow_mut() = None); // a cut is one-shot
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
    apply_style_range_pos(sheet, r0, c0, r1, c1, move |_, _, st| edit(st))
}

/// Like [`apply_style_range`], but the closure also receives the cell's
/// `(row, col)` — needed for position-dependent styling such as outer borders.
fn apply_style_range_pos(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    edit: impl Fn(u32, u32, &mut Style),
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
                edit(r, c, &mut style);
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

/// A cell's borders as JSON `{ "l": "style:color", ... }` — one key per present
/// edge (l/r/t/b), value `"<line-style>:<RRGGBB or empty>"`.
fn border_json(b: &Borders) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut edge = |key: &str, e: &Option<BorderEdge>| {
        if let Some(e) = e {
            let color = e.color.as_deref().unwrap_or("");
            parts.push(format!(
                "\"{key}\":{}",
                json_string(&format!("{}:{color}", e.style))
            ));
        }
    };
    edge("l", &b.left);
    edge("r", &b.right);
    edge("t", &b.top);
    edge("b", &b.bottom);
    format!("{{{}}}", parts.join(","))
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
