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
use std::collections::{BTreeSet, HashMap};

use casual_calc_eval::recalculate;
use casual_calc_formula::{CellReference, Expr, parse, shift_references};
use casual_calc_import::import_package;
use casual_calc_layout::table_style::table_style_colors;
use casual_calc_layout::{
    DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, display_color, display_text,
    layout_viewport,
};
use casual_calc_model::{
    AutoFilter, BorderEdge, Borders, Cell, CellComment, CellRange, CellRef, CellValue, CfRule,
    CommentReply, ConditionalFormat, CustomFilter, DataValidation, DefinedName, FilterOp,
    FilterRule, HAlign, Hyperlink, Id, Sheet, SheetId, SheetVisibility, Style, StyleId, Table,
    ThemeTint, Underline, VAlign, VertAlign, Workbook,
};
use casual_calc_render::render_png;
use casual_calc_sdk::{EditOperation, SheetMetadata, WorkbookSession};
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
    let mut session = WorkbookSession::open(bytes.to_vec()).map_err(js)?;
    reapply_filters_after_load(&mut session);
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
        let n = session.workbook().sheets.len();
        let id = SheetId(Id::from_parts(0x5348, 1000 + n as u64));
        let sheet = Sheet::new(id, format!("Sheet{}", n + 1));
        // Undoable, dirties the doc, and recalculates (a new name can resolve a
        // previously-#REF cross-sheet reference).
        session
            .edit(EditOperation::InsertSheet {
                index: n,
                sheet: Box::new(sheet),
            })
            .map_err(js)?;
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
        let wb = session.workbook();
        if wb
            .sheets
            .iter()
            .enumerate()
            .any(|(i, sh)| i != index && sh.name == name)
        {
            return Err(JsError::new("a sheet with that name already exists"));
        }
        if index >= wb.sheets.len() {
            return Ok(());
        }
        // Undoable + dirties the doc; the edit recalculates so cross-sheet
        // formulas pick up (or lose) the renamed target (refs resolve by name).
        session
            .edit(EditOperation::RenameSheet {
                index,
                name: name.to_owned(),
            })
            .map_err(js)?;
        Ok(())
    })
}

/// Delete a sheet (never the last remaining one).
#[wasm_bindgen]
pub fn session_delete_sheet(index: usize) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        if session.workbook().sheets.len() <= 1 {
            return Err(JsError::new("cannot delete the last sheet"));
        }
        if index >= session.workbook().sheets.len() {
            return Ok(());
        }
        // Undoable (restores the whole sheet) + dirties + recalculates so a
        // cross-sheet reference onto the deleted sheet becomes #REF!.
        session
            .edit(EditOperation::RemoveSheet { index })
            .map_err(js)?;
        Ok(())
    })
}

/// Move a sheet from index `from` to index `to` (tab reorder).
#[wasm_bindgen]
pub fn session_move_sheet(from: usize, to: usize) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let len = session.workbook().sheets.len();
        if from >= len || to >= len || from == to {
            return Ok(());
        }
        session
            .edit(EditOperation::MoveSheet { from, to })
            .map_err(js)?;
        Ok(())
    })
}

/// Duplicate a sheet (inserted right after the source), returning its index.
#[wasm_bindgen]
pub fn session_duplicate_sheet(index: usize) -> Result<usize, JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let len = session.workbook().sheets.len();
        let mut clone = match session.workbook().sheets.get(index) {
            Some(src) => src.clone(),
            None => return Err(JsError::new("no such sheet")),
        };
        clone.id = SheetId(Id::from_parts(0x5348, 2000 + len as u64));
        let base = clone.name.clone();
        let mut n = 2;
        let mut name = format!("{base} ({n})");
        while session.workbook().sheets.iter().any(|sh| sh.name == name) {
            n += 1;
            name = format!("{base} ({n})");
        }
        clone.name = name;
        let at = index + 1;
        // Undoable + dirties + recalculates (the new name may resolve refs).
        session
            .edit(EditOperation::InsertSheet {
                index: at,
                sheet: Box::new(clone),
            })
            .map_err(js)?;
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
    session_find_opts(sheet, query, match_case, false, false, false, false)
}

/// Search a sheet — or the whole workbook — with the options a find bar offers.
///
/// - `whole_cell`: the cell must equal the query, not merely contain it.
/// - `in_values`: match what the cell *displays* (Excel's "Values" look-in)
///   rather than what you would type into it. The difference matters: a
///   formula's text is `=B2*C2` while its value is `13.5`, and only one of
///   those is what the user can see.
/// - `all_sheets`: search every sheet, tagging each hit with its sheet index.
#[wasm_bindgen]
pub fn session_find_opts(
    sheet: usize,
    query: &str,
    match_case: bool,
    whole_cell: bool,
    in_values: bool,
    all_sheets: bool,
    wildcards: bool,
) -> String {
    with_session(|s| {
        let wb = s.workbook();
        if query.is_empty() {
            return "[]".to_owned();
        }
        let sheets: Vec<usize> = if all_sheets {
            (0..wb.sheets.len()).collect()
        } else {
            vec![sheet]
        };
        // With wildcards on, `?` and `*` are pattern syntax. Excel matches the
        // whole cell against the pattern, so a substring search wraps it in `*`
        // rather than falling back to a literal `contains`.
        let pattern = if wildcards && !whole_cell {
            format!("*{query}*")
        } else {
            query.to_owned()
        };
        let matches = |haystack: &str| {
            if wildcards {
                return casual_calc_model::wildcard_match(&pattern, haystack, !match_case);
            }
            if whole_cell {
                if match_case {
                    haystack == query
                } else {
                    haystack.eq_ignore_ascii_case(query)
                }
            } else {
                contains_ci(haystack, query, match_case)
            }
        };
        let mut hits = Vec::new();
        for idx in sheets {
            let Some(sh) = wb.sheets.get(idx) else {
                continue;
            };
            for (at, cell) in sh.cells.iter() {
                let haystack = if in_values {
                    display_text(wb, cell)
                } else {
                    cell_input_text(wb, cell)
                };
                if matches(&haystack) {
                    hits.push(format!(
                        "{{\"r\":{},\"c\":{},\"s\":{}}}",
                        at.row, at.col, idx
                    ));
                }
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
        // Collect (cell, replaced-input) over the editable text of every cell —
        // text, numbers, and formulas alike — so what Find matches, Replace
        // rewrites.
        let mut edits: Vec<(CellRef, String)> = Vec::new();
        if let Some(sh) = session.workbook().sheets.get(sheet) {
            let wb = session.workbook();
            for (at, c) in sh.cells.iter() {
                let input = cell_input_text(wb, c);
                if !contains_ci(&input, find, match_case) {
                    continue;
                }
                let replaced = if match_case {
                    input.replace(find, replace)
                } else {
                    ci_replace(&input, find, replace)
                };
                edits.push((at, replaced));
            }
        }
        let count = edits.len();
        // Re-parse each replaced input so numbers/formulas are re-typed, not
        // frozen as text.
        let ops: Vec<EditOperation> = edits
            .into_iter()
            .map(|(at, input)| build_set_op(session, sheet, at, &input))
            .collect();
        if !ops.is_empty() {
            session.edit(EditOperation::Batch(ops)).map_err(js)?;
        }
        Ok(count)
    })
}

/// Replace occurrences of `find` in a single cell's editable text, re-parsing
/// the result (formula/number/text). Returns whether the cell changed. Used by
/// the Find bar's one-at-a-time **Replace**.
#[wasm_bindgen]
pub fn session_replace_at(
    sheet: usize,
    row: u32,
    col: u32,
    find: &str,
    replace: &str,
    match_case: bool,
) -> Result<bool, JsError> {
    if find.is_empty() {
        return Ok(false);
    }
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let at = CellRef::new(row, col);
        let input = {
            let wb = session.workbook();
            match wb.sheets.get(sheet).and_then(|s| s.cells.get(at)) {
                Some(c) => cell_input_text(wb, c),
                None => return Ok(false),
            }
        };
        if !contains_ci(&input, find, match_case) {
            return Ok(false);
        }
        let replaced = if match_case {
            input.replace(find, replace)
        } else {
            ci_replace(&input, find, replace)
        };
        let op = build_set_op(session, sheet, at, &replaced);
        session.edit(op).map_err(js)?;
        Ok(true)
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

/// Whether a sheet hides its grid lines.
#[wasm_bindgen]
pub fn session_gridlines_hidden(sheet: usize) -> bool {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .is_some_and(|sh| sh.view.hide_gridlines)
    })
    .unwrap_or(false)
}

/// Show or hide a sheet's grid lines (undoable). Returns the new hidden state.
#[wasm_bindgen]
pub fn session_set_gridlines_hidden(sheet: usize, hidden: bool) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let view = &mut data.view;
            view.hide_gridlines = hidden;
        }
        session.edit(op).map_err(js)
    })
}

/// Whether a sheet hides its row and column headers.
#[wasm_bindgen]
pub fn session_headers_hidden(sheet: usize) -> bool {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .is_some_and(|sh| sh.view.hide_headers)
    })
    .unwrap_or(false)
}

/// Show or hide a sheet's row and column headers (undoable). Persisted as
/// OOXML's `showRowColHeaders`, so the choice survives a save.
#[wasm_bindgen]
pub fn session_set_headers_hidden(sheet: usize, hidden: bool) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let view = &mut data.view;
            view.hide_headers = hidden;
        }
        session.edit(op).map_err(js)
    })
}

/// Set the number of frozen rows/columns on a sheet.
#[wasm_bindgen]
pub fn session_set_freeze(sheet: usize, rows: u32, cols: u32) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let view = &mut data.view;
            view.frozen_rows = rows;
            view.frozen_cols = cols;
        }
        session.edit(op).map_err(js)
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
        if session.workbook().sheets.get(sheet).is_none() {
            return Ok(());
        }
        session
            .edit(EditOperation::SetTabColor { sheet, color })
            .map_err(js)
    })
}

/// Add a dropdown-list data-validation rule over a range. Any existing rule
/// intersecting the range is dropped first so a cell has at most one list.
#[wasm_bindgen]
pub fn session_set_list_validation(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    values: Vec<String>,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
        data.validations.retain(|v| {
            !(v.range.start.row <= rr1
                && v.range.end.row >= rr0
                && v.range.start.col <= cc1
                && v.range.end.col >= cc0)
        });
        let clean: Vec<String> = values
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if !clean.is_empty() {
            data.validations.push(DataValidation::list(
                CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1)),
                clean,
            ));
        }
    })
}

/// The dropdown values for the validation covering `(row, col)` as a JSON array,
/// or `null` if the cell has no list validation.
#[wasm_bindgen]
pub fn session_validation_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        // Only a list rule has anything to pick from. A number or date rule
        // returned an empty array, which the host read as "there is a dropdown"
        // — every JS array is truthy — so a whole-number cell grew a chevron
        // that opened onto nothing.
        match sh
            .validations
            .iter()
            .find(|v| v.covers(row, col))
            .filter(|v| v.kind == casual_calc_model::DvKind::List && !v.values.is_empty())
            // `showDropDown="1"` *hides* the in-cell list, as the schema
            // defines it. A file that asked for a typed-only list was still
            // getting a chevron.
            .filter(|v| !v.hide_dropdown)
        {
            Some(v) => {
                let items: Vec<String> = v.values.iter().map(|x| json_string(x)).collect();
                format!("[{}]", items.join(","))
            }
            None => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// Why `input` is not allowed in `(row, col)`, or an empty string if it is.
///
/// A dropdown that accepts anything typed over it is not a validation — it is a
/// suggestion. The host calls this before committing an edit and refuses the
/// commit with the returned message, which is how Excel behaves (and, like
/// Excel, only for typed entry: fill and paste are not gated).
///
/// An empty input always passes: clearing a cell is not entering a bad value.
///
/// Returns `""` when the value is allowed, otherwise JSON
/// `{"style":"stop"|"warning"|"information","title":…,"text":…}`.
///
/// The style matters and used to be dropped: only `stop` refuses the entry.
/// `warning` asks whether to keep it and `information` merely says so — turning
/// either into a hard block is a different rule from the one the author wrote,
/// and there is no way for the user to get past it.
#[wasm_bindgen]
pub fn session_validation_error(sheet: usize, row: u32, col: u32, input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let out = with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return String::new();
        };
        let Some(rule) = sh.validations.iter().find(|v| v.covers(row, col)) else {
            return String::new();
        };
        // The model decides; this only phrases the refusal. `None` means the
        // rule needs the formula engine, so nothing is blocked on it.
        let number = trimmed.parse::<f64>().ok();
        if rule.accepts(trimmed, number) != Some(false) {
            return String::new();
        }
        // Author-set wording always wins: they know what the rule is for.
        if !rule.error_text.is_empty() {
            return rule.error_text.clone();
        }
        if rule.kind == casual_calc_model::DvKind::List {
            let shown: Vec<&str> = rule.values.iter().take(6).map(String::as_str).collect();
            let ellipsis = if rule.values.len() > shown.len() {
                ", …"
            } else {
                ""
            };
            return format!("must be one of: {}{ellipsis}", shown.join(", "));
        }
        let what = match rule.kind {
            casual_calc_model::DvKind::Whole => "a whole number",
            casual_calc_model::DvKind::Decimal => "a number",
            casual_calc_model::DvKind::Date => "a date",
            casual_calc_model::DvKind::Time => "a time",
            casual_calc_model::DvKind::TextLength => "text of an allowed length",
            _ => "a permitted value",
        };
        let bound = match rule.operator {
            casual_calc_model::DvOperator::Between => {
                format!(" between {} and {}", rule.formula1, rule.formula2)
            }
            casual_calc_model::DvOperator::NotBetween => {
                format!(" outside {} to {}", rule.formula1, rule.formula2)
            }
            casual_calc_model::DvOperator::Equal => format!(" equal to {}", rule.formula1),
            casual_calc_model::DvOperator::NotEqual => format!(" not equal to {}", rule.formula1),
            casual_calc_model::DvOperator::GreaterThan => {
                format!(" greater than {}", rule.formula1)
            }
            casual_calc_model::DvOperator::LessThan => format!(" less than {}", rule.formula1),
            casual_calc_model::DvOperator::GreaterThanOrEqual => {
                format!(" at least {}", rule.formula1)
            }
            casual_calc_model::DvOperator::LessThanOrEqual => {
                format!(" at most {}", rule.formula1)
            }
        };
        format!(
            "must be {what}{}",
            if rule.formula1.is_empty() {
                String::new()
            } else {
                bound
            }
        )
    })
    .unwrap_or_default();
    if out.is_empty() {
        return out;
    }
    let (style, title) = with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.validations.iter().find(|v| v.covers(row, col)))
            .map(|r| {
                (
                    r.error_style.clone().unwrap_or_else(|| "stop".to_owned()),
                    r.error_title.clone(),
                )
            })
    })
    .flatten()
    .unwrap_or_else(|| ("stop".to_owned(), String::new()));
    format!(
        "{{\"style\":{},\"title\":{},\"text\":{}}}",
        json_string(&style),
        json_string(&title),
        json_string(&out)
    )
}

/// The input hint on a cell — Excel's "Input Message" — as JSON
/// `{"title":…,"text":…}`, or `""` where the cell has none.
///
/// Shown when the cell is selected, which is the whole point of it: a rule that
/// only speaks up after you have typed something wrong explains the constraint
/// too late.
#[wasm_bindgen]
pub fn session_validation_prompt(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let rule = s
            .workbook()
            .sheets
            .get(sheet)?
            .validations
            .iter()
            .find(|v| v.covers(row, col))?;
        if rule.prompt_title.is_empty() && rule.prompt_text.is_empty() {
            return None;
        }
        Some(format!(
            "{{\"title\":{},\"text\":{}}}",
            json_string(&rule.prompt_title),
            json_string(&rule.prompt_text)
        ))
    })
    .flatten()
    .unwrap_or_default()
}

/// The wording and flags on the rule covering a cell, as JSON, or `""` when the
/// cell has no rule. The panel loads this so editing a rule keeps the author's
/// wording instead of blanking it on the next Apply.
#[wasm_bindgen]
pub fn session_validation_messages(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let r = s
            .workbook()
            .sheets
            .get(sheet)?
            .validations
            .iter()
            .find(|v| v.covers(row, col))?;
        Some(format!(
            "{{\"style\":{},\"errorTitle\":{},\"errorText\":{},\
             \"promptTitle\":{},\"promptText\":{},\"hideDropdown\":{}}}",
            json_string(r.error_style.as_deref().unwrap_or("stop")),
            json_string(&r.error_title),
            json_string(&r.error_text),
            json_string(&r.prompt_title),
            json_string(&r.prompt_text),
            r.hide_dropdown,
        ))
    })
    .flatten()
    .unwrap_or_default()
}

/// Set the messages and the dropdown flag on the rules covering a range,
/// leaving the rule itself alone.
///
/// Separate from `session_set_validation` because they are separate decisions:
/// Excel has an "Input Message" and an "Error Alert" tab precisely so wording
/// can be changed without redefining what is allowed.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_validation_messages(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    style: &str,
    titles: Vec<String>,
    hide_dropdown: bool,
) -> Result<(), JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    // `titles` is [error title, error text, prompt title, prompt text] — four
    // strings of the same kind, which read worse as four positional arguments
    // than as the list they are.
    let get = |i: usize| titles.get(i).cloned().unwrap_or_default();
    let (et, ex, pt, px) = (get(0), get(1), get(2), get(3));
    let style = style.to_owned();
    edit_sheet_metadata(sheet, move |_, data| {
        for v in data.validations.iter_mut() {
            if v.range.start.row <= rr1
                && v.range.end.row >= rr0
                && v.range.start.col <= cc1
                && v.range.end.col >= cc0
            {
                // `stop` is the schema default, so writing it back as `None`
                // keeps an untouched file byte-identical.
                v.error_style = (!style.is_empty() && style != "stop").then(|| style.clone());
                v.error_title = et.clone();
                v.error_text = ex.clone();
                v.prompt_title = pt.clone();
                v.prompt_text = px.clone();
                v.hide_dropdown = hide_dropdown;
            }
        }
    })
}

/// Set a non-list validation over a range: `kind` and `op` are the OOXML tokens,
/// `f1`/`f2` the operands, plus the author's own message wording.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_validation(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
    op: &str,
    f1: &str,
    f2: &str,
    allow_blank: bool,
    error_text: &str,
) -> Result<(), JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    edit_sheet_metadata(sheet, move |_, data| {
        // Replace whatever covered this block, as the list setter does.
        data.validations.retain(|v| {
            !(v.range.start.row <= rr1
                && v.range.end.row >= rr0
                && v.range.start.col <= cc1
                && v.range.end.col >= cc0)
        });
        let range = CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1));
        data.validations.push(casual_calc_model::DataValidation {
            kind: casual_calc_model::DvKind::from_ooxml(kind),
            operator: casual_calc_model::DvOperator::from_ooxml(op),
            formula1: f1.trim().to_owned(),
            formula2: f2.trim().to_owned(),
            allow_blank,
            error_text: error_text.trim().to_owned(),
            ..casual_calc_model::DataValidation::none(range)
        });
    })
}

/// Remove any validation intersecting a range.
#[wasm_bindgen]
pub fn session_clear_validation(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        data.validations.retain(|v| {
            !(v.range.start.row <= r1
                && v.range.end.row >= r0
                && v.range.start.col <= c1
                && v.range.end.col >= c0)
        });
    })
}

/// Add a highlight-cells conditional-format rule over a range. `kind` is one of
/// `gt`/`lt`/`eq`/`between`/`contains`; `a`/`b` are numeric operands (b only for
/// `between`), `text` the substring for `contains`, `fill` the `RRGGBB` color.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_add_cf(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
    a: f64,
    b: f64,
    text: &str,
    fill: &str,
) -> Result<(), JsError> {
    let rule = match kind {
        "gt" => CfRule::GreaterThan(a),
        "lt" => CfRule::LessThan(a),
        "eq" => CfRule::EqualTo(a),
        "between" => CfRule::Between(a.min(b), a.max(b)),
        "contains" => CfRule::TextContains(text.to_owned()),
        // Range-relative kinds take their colours through `text` as a
        // comma-separated list (low → high), since they need two or three and
        // the single `fill` slot cannot carry them.
        "colorscale" => {
            let colors: Vec<String> = text
                .split(',')
                .map(|c| c.trim().trim_start_matches('#').to_ascii_uppercase())
                .filter(|c| c.len() == 6)
                .collect();
            if colors.len() < 2 {
                return Err(JsError::new("a colour scale needs at least two colours"));
            }
            CfRule::ColorScale(colors)
        }
        "databar" => CfRule::DataBar(text.trim().trim_start_matches('#').to_ascii_uppercase()),
        // Ranked / statistical kinds: the operand `a` is the rank where one
        // applies, and a rank of zero would select nothing.
        "top" | "bottom" | "toppct" | "bottompct" => CfRule::Top10 {
            rank: (a as u32).max(1),
            bottom: kind.starts_with("bottom"),
            percent: kind.ends_with("pct"),
        },
        "above" | "below" => CfRule::AboveAverage {
            below: kind == "below",
            equal: false,
        },
        "duplicate" => CfRule::DuplicateValues { unique: false },
        "unique" => CfRule::DuplicateValues { unique: true },
        _ => return Err(JsError::new("unknown conditional-format rule")),
    };
    let fill = fill.trim().trim_start_matches('#').to_ascii_uppercase();
    edit_sheet_metadata(sheet, move |_, data| {
        let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
        // New rules go last in priority, so they do not silently outrank the
        // ones already there.
        let next = data
            .conditional_formats
            .iter()
            .map(|c| c.priority)
            .max()
            .unwrap_or(0)
            + 1;
        let mut cf = ConditionalFormat::new(
            CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1)),
            rule,
            fill,
        );
        cf.priority = next;
        data.conditional_formats.push(cf);
    })
}

/// Remove every conditional-format rule intersecting a range.
#[wasm_bindgen]
pub fn session_clear_cf(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        data.conditional_formats.retain(|cf| {
            !(cf.range.start.row <= r1
                && cf.range.end.row >= r0
                && cf.range.start.col <= c1
                && cf.range.end.col >= c0)
        });
    })
}

/// Set (or, with empty text, remove) a cell's comment. Replaces the whole
/// thread, so any replies go with it — this is the "edit the note" path.
///
/// `author` and `created` may be empty, which leaves a plain note. `created` is
/// passed in as an ISO 8601 string rather than read from a clock here so the
/// core stays deterministic: the same sequence of edits produces the same bytes.
#[wasm_bindgen]
pub fn session_set_comment(
    sheet: usize,
    row: u32,
    col: u32,
    text: &str,
    author: &str,
    created: &str,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        // Editing keeps the replies that were already on the thread; only
        // an empty text (a delete) drops them.
        let existing = data
            .comments
            .iter()
            .position(|c| c.at.row == row && c.at.col == col);
        let text = text.trim();
        if text.is_empty() {
            if let Some(i) = existing {
                data.comments.remove(i);
            }
            return;
        }
        let mut thread = match existing {
            Some(i) => data.comments.remove(i),
            None => CellComment::note(CellRef::new(row, col), "", None),
        };
        thread.text = text.to_owned();
        if !author.is_empty() {
            thread.author = Some(author.to_owned());
        }
        if !created.is_empty() {
            thread.created = Some(created.to_owned());
        }
        data.comments.push(thread);
    })
}

/// Append a reply to the thread on a cell. A no-op if the cell has no thread —
/// a reply without an opening remark has nothing to attach to.
#[wasm_bindgen]
pub fn session_reply_comment(
    sheet: usize,
    row: u32,
    col: u32,
    text: &str,
    author: &str,
    created: &str,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(thread) = data
            .comments
            .iter_mut()
            .find(|c| c.at.row == row && c.at.col == col)
        {
            thread.replies.push(CommentReply {
                text: text.to_owned(),
                author: (!author.is_empty()).then(|| author.to_owned()),
                created: (!created.is_empty()).then(|| created.to_owned()),
            });
        }
    })
}

/// Mark a cell's thread resolved or reopened.
#[wasm_bindgen]
pub fn session_resolve_comment(
    sheet: usize,
    row: u32,
    col: u32,
    resolved: bool,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(thread) = data
            .comments
            .iter_mut()
            .find(|c| c.at.row == row && c.at.col == col)
        {
            thread.resolved = resolved;
        }
    })
}

/// Set (or, with an empty target and location, remove) the hyperlink on a cell.
///
/// `target` is an external URI and `location` an anchor inside this workbook;
/// either may be empty, and a link with both means "open that document at this
/// anchor". Goes through the metadata log, so it is undoable like any edit.
#[wasm_bindgen]
pub fn session_set_hyperlink(
    sheet: usize,
    row: u32,
    col: u32,
    target: &str,
    location: &str,
    tooltip: &str,
    display: &str,
) -> Result<(), JsError> {
    let target = target.trim().to_owned();
    let location = location.trim().to_owned();
    let tooltip = tooltip.trim().to_owned();
    let display = display.trim().to_owned();
    edit_sheet_metadata(sheet, move |_, data| {
        data.hyperlinks
            .retain(|h| !(h.range.start.row == row && h.range.start.col == col));
        // Neither destination means "remove": a link with nowhere to go would
        // render as a live link that does nothing.
        if target.is_empty() && location.is_empty() {
            return;
        }
        data.hyperlinks.push(Hyperlink {
            range: CellRange::new(CellRef::new(row, col), CellRef::new(row, col)),
            target: (!target.is_empty()).then_some(target),
            location: (!location.is_empty()).then_some(location),
            tooltip: (!tooltip.is_empty()).then_some(tooltip),
            display: (!display.is_empty()).then_some(display),
        });
    })
}

/// The hyperlink covering a cell as JSON, or `null`.
#[wasm_bindgen]
pub fn session_hyperlink_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(link) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.hyperlinks.iter().find(|h| {
                row >= h.range.start.row
                    && row <= h.range.end.row
                    && col >= h.range.start.col
                    && col <= h.range.end.col
            })
        }) else {
            return "null".to_owned();
        };
        let field = |v: &Option<String>| v.as_deref().map_or("null".to_owned(), json_string);
        format!(
            "{{\"target\":{},\"location\":{},\"tooltip\":{},\"display\":{}}}",
            field(&link.target),
            field(&link.location),
            field(&link.tooltip),
            field(&link.display),
        )
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The linked cells within a range as JSON `[{r,c}, …]`, so the grid can
/// underline them without asking cell by cell.
#[wasm_bindgen]
pub fn session_hyperlink_cells(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let mut items = Vec::new();
        for link in &sh.hyperlinks {
            for r in link.range.start.row..=link.range.end.row {
                for c in link.range.start.col..=link.range.end.col {
                    if r >= r0 && r <= r1 && c >= c0 && c <= c1 {
                        items.push(format!("{{\"r\":{r},\"c\":{c}}}"));
                    }
                }
            }
        }
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The contiguous block of populated cells around `(row, col)`, as JSON
/// `{r0,c0,r1,c1}`, or `null` when the cell is empty.
///
/// What Ctrl+T uses when the selection is a single cell: asking someone to
/// select the whole table first is work the app can do, and doing it here means
/// the same rule applies wherever a block is needed.
#[wasm_bindgen]
pub fn session_block_bounds(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        let filled = |r: u32, c: u32| {
            sh.cells
                .get(CellRef::new(r, c))
                .is_some_and(|cell| !cell.value.is_empty() || cell.formula.is_some())
        };
        if !filled(row, col) {
            return "null".to_owned();
        }
        // Walk out along the row and column, then square the block off. A
        // ragged region grows to its bounding box, which is what a user means
        // by "this table" even when one corner is blank.
        let (mut r0, mut r1, mut c0, mut c1) = (row, row, col, col);
        while r0 > 0 && (c0..=c1).any(|c| filled(r0 - 1, c)) {
            r0 -= 1;
        }
        while c0 > 0 && (r0..=r1).any(|r| filled(r, c0 - 1)) {
            c0 -= 1;
        }
        // Bounded so a pathological sheet cannot make this walk forever.
        let limit = 1_048_576u32;
        while r1 + 1 < limit && (c0..=c1).any(|c| filled(r1 + 1, c)) {
            r1 += 1;
        }
        while c1 + 1 < 16_384 && (r0..=r1).any(|r| filled(r, c1 + 1)) {
            c1 += 1;
        }
        format!("{{\"r0\":{r0},\"c0\":{c0},\"r1\":{r1},\"c1\":{c1}}}")
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The range a filter should actually cover for a selection, as JSON
/// `{r0,c0,r1,c1}`, or `null` when there is nothing to filter.
///
/// A selection that is genuinely two-dimensional is taken as given. Anything
/// thinner — one cell, one row, one column, the whole sheet — is grown to the
/// contiguous block around the first populated cell inside it, which is what
/// Excel does and what the user means.
///
/// Without this, the ordinary way to reach the feature broke it: clicking the
/// row 1 header and pressing Filter asks, literally, for one row spanning all
/// 16384 columns. That is a filter with no rows beneath its header — every
/// checklist empty — and a button on every column of the sheet, blank ones
/// included.
#[wasm_bindgen]
pub fn session_filter_range_for(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    let (r0, c0, r1, c1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        let filled = |r: u32, c: u32| {
            sh.cells
                .get(CellRef::new(r, c))
                .is_some_and(|cell| !cell.value.is_empty() || cell.formula.is_some())
        };
        if r1 > r0 && c1 > c0 {
            return format!("{{\"r0\":{r0},\"c0\":{c0},\"r1\":{r1},\"c1\":{c1}}}");
        }
        // Scanning the populated cells rather than the box: a whole-row
        // selection is 16384 columns wide and all but a handful are empty.
        let seed = sh
            .cells
            .iter()
            .map(|(at, _)| at)
            .filter(|at| at.row >= r0 && at.row <= r1 && at.col >= c0 && at.col <= c1)
            .filter(|at| filled(at.row, at.col))
            .min_by_key(|at| (at.row, at.col));
        match seed {
            Some(at) => session_block_bounds(sheet, at.row, at.col),
            None => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// Create a table over a range — Excel's Ctrl+T.
///
/// The header row's cells become the column names, because a structured
/// reference resolves by name: `Sales[Amount]` finds its column through the
/// header text, so a table whose columns disagree with their headers has
/// formulas pointing at nothing. Empty or duplicate headers are filled in, for
/// the same reason.
#[wasm_bindgen]
pub fn session_create_table(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    name: &str,
    has_headers: bool,
) -> Result<String, JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    // A name must be unique across the workbook: structured references are
    // resolved by name alone, so two tables sharing one makes every reference
    // to it ambiguous.
    let taken: Vec<String> = with_session(|s| {
        s.workbook()
            .sheets
            .iter()
            .flat_map(|sh| sh.tables.iter().map(|t| t.name.to_ascii_lowercase()))
            .collect()
    })
    .unwrap_or_default();
    let base = {
        let trimmed = name.trim();
        if trimmed.is_empty() { "Table" } else { trimmed }
    };
    let mut final_name = base.to_owned();
    let mut n = 1;
    while taken.contains(&final_name.to_ascii_lowercase()) {
        n += 1;
        final_name = format!("{base}{n}");
    }

    // Column names come from the header cells when there is a header row.
    let headers: Vec<String> = with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return Vec::new();
        };
        (cc0..=cc1)
            .map(|c| {
                if !has_headers {
                    return String::new();
                }
                sh.cells
                    .get(CellRef::new(rr0, c))
                    .map(|cell| value_text(s.workbook(), &cell.value))
                    .unwrap_or_default()
            })
            .collect()
    })
    .unwrap_or_default();

    let mut names: Vec<String> = Vec::new();
    for (i, header) in headers.iter().enumerate() {
        let mut candidate = header.trim().to_owned();
        if candidate.is_empty() {
            candidate = format!("Column{}", i + 1);
        }
        // Duplicates get a suffix rather than being left to collide: a
        // reference to a duplicated name would resolve to whichever came first,
        // silently reading the wrong column.
        let mut unique = candidate.clone();
        let mut k = 1;
        while names
            .iter()
            .any(|n: &String| n.eq_ignore_ascii_case(&unique))
        {
            k += 1;
            unique = format!("{candidate}{k}");
        }
        names.push(unique);
    }

    let id = with_session(|s| {
        s.workbook()
            .sheets
            .iter()
            .flat_map(|sh| sh.tables.iter().map(|t| t.id))
            .max()
            .unwrap_or(0)
            + 1
    })
    .unwrap_or(1);

    let created = final_name.clone();
    let columns: Vec<casual_calc_model::TableColumn> = names
        .into_iter()
        .enumerate()
        .map(|(i, n)| casual_calc_model::TableColumn {
            id: i as u32 + 1,
            name: n,
            totals_row_function: None,
            totals_row_label: None,
            calculated_column_formula: None,
            totals_row_formula: None,
        })
        .collect();
    edit_sheet_metadata(sheet, move |_, data| {
        data.tables.push(Table {
            id,
            name: final_name.clone(),
            display_name: final_name,
            range: CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1)),
            header_row_count: u32::from(has_headers),
            totals_row_count: 0,
            columns,
            // Excel turns the filter buttons on with the table; without them
            // the header row looks like an ordinary row that happens to be
            // shaded.
            auto_filter: Some(AutoFilter::new(CellRange::new(
                CellRef::new(rr0, cc0),
                CellRef::new(rr1, cc1),
            ))),
            style: [
                ("name".to_owned(), "TableStyleMedium2".to_owned()),
                ("showRowStripes".to_owned(), "1".to_owned()),
            ]
            .into_iter()
            .collect(),
            attrs: Default::default(),
        });
    })?;
    Ok(created)
}

/// Remove the table covering a cell, leaving its values in place — Excel's
/// "Convert to Range".
#[wasm_bindgen]
pub fn session_remove_table(sheet: usize, row: u32, col: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        data.tables.retain(|t| {
            !(row >= t.range.start.row
                && row <= t.range.end.row
                && col >= t.range.start.col
                && col <= t.range.end.col)
        });
    })
}

/// The `SUBTOTAL` function number for a `totalsRowFunction` name.
///
/// The 10x codes ignore rows the filter has hidden, which is the whole point of
/// a table total: filter to one region and the total follows. Excel writes the
/// same codes.
fn totals_subtotal_code(func: &str) -> Option<u32> {
    Some(match func {
        "average" => 101,
        "count" => 103,
        "countNums" => 102,
        "max" => 104,
        "min" => 105,
        "stdDev" => 107,
        "sum" => 109,
        "var" => 110,
        _ => return None,
    })
}

/// Set a column's totals-row function, writing the formula the choice means.
///
/// Excel stores both: `totalsRowFunction="sum"` on the column *and* a real
/// `SUBTOTAL(109, Table[Column])` in the cell. Recording only the attribute —
/// which is all the model did — leaves the totals row blank on screen and in
/// every other reader; writing only the formula loses the choice on save. The
/// two go together, in one undo step, or an undo leaves them disagreeing.
///
/// `func` is an OOXML name (`sum`, `average`, `count`, `countNums`, `max`,
/// `min`, `stdDev`, `var`) or empty to clear the cell back to nothing.
#[wasm_bindgen]
pub fn session_set_totals_function(
    sheet: usize,
    row: u32,
    col: u32,
    func: &str,
) -> Result<(), JsError> {
    let func = func.to_owned();
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
            return Ok(());
        };
        let Some(t) = sh.tables.iter().find(|t| {
            row >= t.range.start.row
                && row <= t.range.end.row
                && col >= t.range.start.col
                && col <= t.range.end.col
        }) else {
            return Ok(());
        };
        if t.totals_row_count == 0 {
            return Err(JsError::new("this table has no totals row"));
        }
        let Some(index) = (col.checked_sub(t.range.start.col)).map(|i| i as usize) else {
            return Ok(());
        };
        let Some(column) = t.columns.get(index) else {
            return Ok(());
        };
        let at = CellRef::new(t.range.end.row, col);
        // The structured reference, not an A1 range: inserting a row into the
        // table has to widen the total, and only the name does that.
        let text = match totals_subtotal_code(&func) {
            Some(code) => format!("=SUBTOTAL({code},{}[{}])", t.name, column.name),
            None => String::new(),
        };

        let mut data = SheetMetadata::capture(&sh);
        if let Some(t) = table_at_mut(&mut data.tables, row, col)
            && let Some(c) = t.columns.get_mut(index)
        {
            c.totals_row_function = (!func.is_empty()).then(|| func.clone());
            // A label and a function are alternatives on the same cell: Excel
            // writes one or the other, never both.
            if !func.is_empty() {
                c.totals_row_label = None;
            }
        }
        let cell_op = build_set_op(session, sheet, at, &text);
        session
            .edit(EditOperation::Batch(vec![
                EditOperation::SetSheetMetadata {
                    sheet,
                    data: Box::new(data),
                },
                cell_op,
            ]))
            .map_err(js)
    })
}

/// The totals-row function on each column of the table under a cell, as a JSON
/// array of names (empty string where a column has none).
#[wasm_bindgen]
pub fn session_totals_functions(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(t) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.tables.iter().find(|t| {
                row >= t.range.start.row
                    && row <= t.range.end.row
                    && col >= t.range.start.col
                    && col <= t.range.end.col
            })
        }) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = t
            .columns
            .iter()
            .map(|c| json_string(c.totals_row_function.as_deref().unwrap_or_default()))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Rename the table under a cell.
///
/// A structured reference resolves by name alone, so the new name has to be
/// unique across the workbook or `Sales[Amount]` starts pointing at whichever
/// table the resolver reaches first. A clash is rejected rather than silently
/// suffixed: the user typed a specific name and deserves to be told.
#[wasm_bindgen]
pub fn session_rename_table(sheet: usize, row: u32, col: u32, name: &str) -> Result<(), JsError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(JsError::new("a table needs a name"));
    }
    // Excel's rule: a name is an identifier, not a label — no spaces, and it
    // cannot look like a cell reference.
    if name.chars().next().is_some_and(|c| c.is_ascii_digit())
        || name
            .chars()
            .any(|c| !(c.is_alphanumeric() || c == '_' || c == '.'))
    {
        return Err(JsError::new(
            "a table name must start with a letter and hold only letters, digits, _ or .",
        ));
    }
    let clash = with_session(|s| {
        s.workbook().sheets.iter().enumerate().any(|(i, sh)| {
            sh.tables.iter().any(|t| {
                t.name.eq_ignore_ascii_case(&name)
                    && !(i == sheet
                        && row >= t.range.start.row
                        && row <= t.range.end.row
                        && col >= t.range.start.col
                        && col <= t.range.end.col)
            })
        })
    })
    .unwrap_or(false);
    if clash {
        return Err(JsError::new("another table already has that name"));
    }
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            t.name = name.clone();
            t.display_name = name.clone();
        }
    })
}

/// Set the style name and banding flags on the table under a cell.
///
/// The name is what every colour is derived from — Excel stores no fills for a
/// table, only this name — so changing it is what restyles the table.
///
/// `flags` is a bitmask: 1 banded rows, 2 banded columns, 4 emphasise the first
/// column, 8 emphasise the last. One argument rather than four booleans so the
/// whole change stays a single undo step.
#[wasm_bindgen]
pub fn session_set_table_style(
    sheet: usize,
    row: u32,
    col: u32,
    style: &str,
    flags: u32,
) -> Result<(), JsError> {
    let style = style.to_owned();
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            for (bit, key) in [
                (1, "showRowStripes"),
                (2, "showColumnStripes"),
                (4, "showFirstColumn"),
                (8, "showLastColumn"),
            ] {
                t.style
                    .insert(key.to_owned(), u8::from(flags & bit != 0).to_string());
            }
            if style.is_empty() {
                t.style.remove("name");
            } else {
                t.style.insert("name".to_owned(), style.clone());
            }
        }
    })
}

/// Turn a table's header row on or off.
///
/// The range does not move: Excel's "Header Row" checkbox decides whether the
/// table's first row is read as headers, not where the table sits. Shifting the
/// range here would either swallow a row of data or leave one stranded outside
/// the table.
#[wasm_bindgen]
pub fn session_set_table_headers(
    sheet: usize,
    row: u32,
    col: u32,
    on: bool,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            t.header_row_count = u32::from(on);
        }
    })
}

/// The colours a style name resolves to, as JSON — what the style picker
/// paints its swatches with, so the preview and the grid cannot disagree.
#[wasm_bindgen]
pub fn session_table_style_preview(style: &str) -> String {
    with_session(|s| {
        let c = table_style_colors(s.workbook(), style);
        format!(
            "{{\"headerFill\":{},\"headerText\":{},\"bodyFill\":{},\"bandFill\":{},\"border\":{}}}",
            json_string(&c.header_fill),
            json_string(&c.header_text),
            json_string(&c.body_fill),
            json_string(&c.band_fill),
            json_string(&c.border),
        )
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The table covering a cell, mutably. Every table command needs this same
/// lookup, and writing it out each time is how two of them drifted apart.
fn table_at_mut(tables: &mut [Table], row: u32, col: u32) -> Option<&mut Table> {
    tables.iter_mut().find(|t| {
        row >= t.range.start.row
            && row <= t.range.end.row
            && col >= t.range.start.col
            && col <= t.range.end.col
    })
}

/// Turn a table's totals row on or off, growing or shrinking its range.
#[wasm_bindgen]
pub fn session_table_totals(sheet: usize, row: u32, col: u32, on: bool) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
            return Ok(());
        };
        let Some(t) = sh.tables.iter().find(|t| {
            row >= t.range.start.row
                && row <= t.range.end.row
                && col >= t.range.start.col
                && col <= t.range.end.col
        }) else {
            return Ok(());
        };
        if (t.totals_row_count > 0) == on {
            return Ok(());
        }
        let first_col = t.range.start.col;
        let last_col = t.range.end.col;
        // Turning it on adds a row below; turning it off gives back the one it
        // occupied, so the cells to write are on different rows in each case.
        let totals_row = if on {
            t.range.end.row + 1
        } else {
            t.range.end.row
        };

        let mut data = SheetMetadata::capture(&sh);
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            t.totals_row_count = u32::from(on);
            // The totals row is *inside* the table's range, so switching it
            // must move the bottom edge — leaving the range alone would make
            // the last data row read as the totals row.
            if on {
                t.range.end.row += 1;
            } else {
                t.range.end.row = t.range.end.row.saturating_sub(1);
            }
            if let Some(c) = t.columns.first_mut() {
                // Excel labels the first column "Total" and leaves the rest for
                // the user to choose a function for.
                c.totals_row_label = on.then(|| "Total".to_owned());
            }
            if !on {
                for c in t.columns.iter_mut() {
                    c.totals_row_function = None;
                    c.totals_row_label = None;
                }
            }
        }

        let mut ops = vec![EditOperation::SetSheetMetadata {
            sheet,
            data: Box::new(data),
        }];
        // Turning the row off has to clear what it held: the range shrinks but
        // the cells do not move, so a stale "Total" would be left sitting under
        // the table looking like data.
        for c in first_col..=last_col {
            let at = CellRef::new(totals_row, c);
            let text = if on && c == first_col { "Total" } else { "" };
            ops.push(build_set_op(session, sheet, at, text));
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Grow the table that a newly-typed cell sits directly below or beside.
///
/// Typing in the row under a table extends it, which is the behaviour that
/// makes a table worth having: the range, the banding and every structured
/// reference follow the data instead of needing to be re-pointed by hand.
///
/// A no-op unless the cell is exactly one row below, or one column right of,
/// a table — growing on anything further away would swallow unrelated data.
#[wasm_bindgen]
pub fn session_table_autoexpand(sheet: usize, row: u32, col: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        for table in data.tables.iter_mut() {
            let bottom = table.range.end.row;
            let within_cols = col >= table.range.start.col && col <= table.range.end.col;
            let within_rows = row >= table.range.start.row && row <= table.range.end.row;
            // A totals row sits at the bottom, so a new data row goes *above*
            // it — growing past it would leave the totals stranded mid-table.
            if within_cols && table.totals_row_count == 0 && row == bottom + 1 {
                table.range.end.row = row;
                // Widen the filter with the table, keeping any rules on it —
                // rebuilding it from the range would silently clear them.
                if let Some(filter) = table.auto_filter.as_mut() {
                    filter.range = table.range;
                }
                return;
            }
            if within_rows && col == table.range.end.col + 1 {
                table.range.end.col = col;
                // A new column needs a name, or a structured reference to it
                // has nothing to resolve against.
                let next = table.columns.len() + 1;
                let mut name = format!("Column{next}");
                let mut k = next;
                while table
                    .columns
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(&name))
                {
                    k += 1;
                    name = format!("Column{k}");
                }
                table.columns.push(casual_calc_model::TableColumn {
                    id: table.columns.len() as u32 + 1,
                    name,
                    totals_row_function: None,
                    totals_row_label: None,
                    calculated_column_formula: None,
                    totals_row_formula: None,
                });
                // Widen the filter with the table, keeping any rules on it —
                // rebuilding it from the range would silently clear them.
                if let Some(filter) = table.auto_filter.as_mut() {
                    filter.range = table.range;
                }
                return;
            }
        }
    })
}

/// One table as JSON, with its style resolved to concrete colours.
///
/// Shared by `session_table_at` and `session_tables`: the two used to format
/// this separately, which is how `showRowStripes` went out as a bare `1` from
/// one of them while the host compared it to the string `"1"` — banding never
/// painted on any table, and nothing pointed at why.
fn table_json(workbook: &Workbook, t: &Table) -> String {
    let flag = |key: &str| {
        matches!(
            t.style.get(key).map(String::as_str),
            Some("1") | Some("true")
        )
    };
    let style = t.style.get("name").map(String::as_str).unwrap_or_default();
    let c = table_style_colors(workbook, style);
    // The column names as the model holds them, which is what a structured
    // reference resolves against — not the header cells' display text, which
    // can differ once a header is edited.
    let cols: Vec<String> = t.columns.iter().map(|c| json_string(&c.name)).collect();
    format!(
        "{{\"name\":{},\"style\":{},\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{},\
         \"headers\":{},\"totals\":{},\"stripes\":{},\"colStripes\":{},\
         \"firstCol\":{},\"lastCol\":{},\
         \"headerFill\":{},\"headerText\":{},\"bodyFill\":{},\"bodyText\":{},\
         \"bandFill\":{},\"border\":{},\"cols\":[{}]}}",
        json_string(&t.name),
        json_string(style),
        t.range.start.row,
        t.range.start.col,
        t.range.end.row,
        t.range.end.col,
        t.header_row_count,
        t.totals_row_count,
        flag("showRowStripes"),
        flag("showColumnStripes"),
        flag("showFirstColumn"),
        flag("showLastColumn"),
        json_string(&c.header_fill),
        json_string(&c.header_text),
        json_string(&c.body_fill),
        json_string(&c.body_text),
        json_string(&c.band_fill),
        json_string(&c.border),
        cols.join(","),
    )
}

/// The table covering a cell as JSON, or `null` — drives the UI's state.
#[wasm_bindgen]
pub fn session_table_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(t) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.tables.iter().find(|t| {
                row >= t.range.start.row
                    && row <= t.range.end.row
                    && col >= t.range.start.col
                    && col <= t.range.end.col
            })
        }) else {
            return "null".to_owned();
        };
        table_json(s.workbook(), t)
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// Every table on a sheet, for painting bands and header buttons in one pass.
#[wasm_bindgen]
pub fn session_tables(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .tables
            .iter()
            .map(|t| table_json(s.workbook(), t))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// A cell's comment text, or `""` if it has none.
#[wasm_bindgen]
pub fn session_comment_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| {
                sh.comments
                    .iter()
                    .find(|c| c.at.row == row && c.at.col == col)
            })
            .map(|c| c.text.clone())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// A cell's whole thread as JSON, or `null` if it has none:
/// `{"text","author","created","resolved",replies:[{"text","author","created"}]}`.
#[wasm_bindgen]
pub fn session_comment_thread(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(thread) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.comments
                .iter()
                .find(|c| c.at.row == row && c.at.col == col)
        }) else {
            return "null".to_owned();
        };
        let entry = |text: &str, author: &Option<String>, created: &Option<String>| {
            format!(
                "{{\"text\":{},\"author\":{},\"created\":{}}}",
                json_string(text),
                author.as_deref().map_or("null".to_owned(), json_string),
                created.as_deref().map_or("null".to_owned(), json_string),
            )
        };
        let replies: Vec<String> = thread
            .replies
            .iter()
            .map(|r| entry(&r.text, &r.author, &r.created))
            .collect();
        format!(
            "{{\"text\":{},\"author\":{},\"created\":{},\"resolved\":{},\"replies\":[{}]}}",
            json_string(&thread.text),
            thread
                .author
                .as_deref()
                .map_or("null".to_owned(), json_string),
            thread
                .created
                .as_deref()
                .map_or("null".to_owned(), json_string),
            thread.resolved,
            replies.join(",")
        )
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The commented cells within a range as JSON `[{r,c}, …]` — the editor draws a
/// note indicator on each.
#[wasm_bindgen]
pub fn session_comments(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .comments
            .iter()
            .filter(|c| c.at.row >= r0 && c.at.row <= r1 && c.at.col >= c0 && c.at.col <= c1)
            .map(|c| format!("{{\"r\":{},\"c\":{}}}", c.at.row, c.at.col))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Define (or replace) a workbook-scoped named range. `refers_to` is a formula
/// such as `Sheet1!A1:B2` or `A1`. Rejects empty names and names that collide
/// with a cell reference. Recalculates so name-using formulas update.
#[wasm_bindgen]
pub fn session_define_name(name: &str, refers_to: &str) -> Result<(), JsError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(JsError::new("name cannot be empty"));
    }
    if casual_calc_formula::parse_a1(name).is_some() {
        return Err(JsError::new("that name looks like a cell reference"));
    }
    let expr = parse(refers_to.trim().trim_start_matches('='))
        .map_err(|e| JsError::new(&e.to_string()))?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut names = session.workbook().defined_names.clone();
        names.retain(|d| d.name != name);
        names.push(DefinedName {
            name: name.to_owned(),
            sheet: None,
            formula: expr,
        });
        // Undoable, dirties the doc, and recalculates (a new/changed name can
        // resolve previously-#NAME? formulas or change what they resolve to).
        session
            .edit(EditOperation::SetDefinedNames(names))
            .map_err(js)
    })
}

/// Delete a defined name (undoable); recalculates so dependents become `#NAME?`.
#[wasm_bindgen]
pub fn session_delete_name(name: &str) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut names = session.workbook().defined_names.clone();
        names.retain(|d| d.name != name);
        session
            .edit(EditOperation::SetDefinedNames(names))
            .map_err(js)
    })
}

/// All defined names as JSON `[{name, refersTo}, …]`.
#[wasm_bindgen]
pub fn session_names() -> String {
    with_session(|s| {
        let items: Vec<String> = s
            .workbook()
            .defined_names
            .iter()
            .map(|d| {
                format!(
                    "{{\"name\":{},\"refersTo\":{}}}",
                    json_string(&d.name),
                    json_string(&d.formula.to_string())
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// The target range of a defined name as JSON `{r0,c0,r1,c1}`, or `null` if the
/// name is unknown or refers to something other than a cell/range.
#[wasm_bindgen]
pub fn session_name_target(name: &str) -> String {
    with_session(|s| {
        let Some(d) = s.workbook().defined_names.iter().find(|d| d.name == name) else {
            return "null".to_owned();
        };
        match &d.formula {
            Expr::Reference(r) => {
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}}}",
                    r.row, r.col, r.row, r.col
                )
            }
            Expr::Range(a, b) => format!(
                "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}}}",
                a.row.min(b.row),
                a.col.min(b.col),
                a.row.max(b.row),
                a.col.max(b.col)
            ),
            _ => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
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
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let merges = &mut data.merges;
            merges.retain(|m| !merge_hits(m, r0, c0, r1, c1));
            if r0 != r1 || c0 != c1 {
                merges.push(CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1)));
            }
        }
        session.edit(op).map_err(js)
    })
}

/// How many cells in a range, other than its top-left, hold a value.
///
/// Merging keeps only the top-left value, so the host asks this first: a
/// non-zero answer means the merge would destroy data and the user has to be
/// told before it happens.
#[wasm_bindgen]
pub fn session_merge_hidden_count(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> u32 {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return 0;
        };
        let mut n = 0;
        for r in r0..=r1 {
            for c in c0..=c1 {
                if r == r0 && c == c0 {
                    continue;
                }
                let occupied = sh
                    .cells
                    .get(CellRef::new(r, c))
                    .is_some_and(|cell| cell.formula.is_some() || cell.value != CellValue::Empty);
                if occupied {
                    n += 1;
                }
            }
        }
        n
    })
    .unwrap_or(0)
}

/// Merge a range **and** clear every value it covers except the top-left, in
/// one undoable step.
///
/// The plain merge only records the range, which leaves the other values in the
/// document: invisible on screen, still in the file, and back again the moment
/// the merge is removed. Excel discards them, and so does this — but only after
/// the host has warned, which is why the destructive form is a separate entry
/// point rather than the default.
#[wasm_bindgen]
pub fn session_merge_cells_discarding(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut merge_op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut merge_op {
            let merges = &mut data.merges;
            merges.retain(|m| !merge_hits(m, r0, c0, r1, c1));
            if r0 != r1 || c0 != c1 {
                merges.push(CellRange::new(CellRef::new(r0, c0), CellRef::new(r1, c1)));
            }
        }
        // Clear the covered cells' values but keep their styling, so the block
        // does not lose its fill or borders along with the text.
        let mut ops = vec![merge_op];
        for r in r0..=r1 {
            for c in c0..=c1 {
                if r == r0 && c == c0 {
                    continue;
                }
                let at = CellRef::new(r, c);
                let existing = session
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|s| s.cells.get(at));
                let Some(existing) = existing else { continue };
                if existing.formula.is_none() && existing.value == CellValue::Empty {
                    continue;
                }
                match existing.style {
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
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let merges = &mut data.merges;
            merges.retain(|m| !merge_hits(m, r0, c0, r1, c1));
        }
        session.edit(op).map_err(js)
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

/// A `SetSheetMetadata` op carrying the sheet's *current* metadata bundle
/// (merges, axis sizing, hidden sets, view, autofilter). Callers tweak one field
/// and submit it so freeze / merge / resize-all / filter become single undoable
/// edits that dirty the document. `None` if the sheet index is out of range.
fn current_sheet_metadata(session: &WorkbookSession, sheet: usize) -> Option<EditOperation> {
    let sh = session.workbook().sheets.get(sheet)?;
    Some(EditOperation::SetSheetMetadata {
        sheet,
        data: Box::new(SheetMetadata::capture(sh)),
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
    // Defer to the model's own parser so this can never drift from the OOXML
    // token set again; "middle" is the host's word for what OOXML calls
    // "center", and an unrecognised token clears back to General.
    let value = VAlign::from_ooxml(if valign == "middle" { "center" } else { valign });
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

/// Whether each sheet is protected, as a JSON array of 0/1.
#[wasm_bindgen]
pub fn session_sheet_protected() -> String {
    with_session(|s| {
        let items: Vec<String> = s
            .workbook()
            .sheets
            .iter()
            .map(|sh| u8::from(sh.protection.as_ref().is_some_and(|p| p.is_enabled())).to_string())
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Turn sheet protection on or off.
///
/// Turning it on sets only the master flag: this cannot invent a password hash,
/// and a UI that pretended to would be claiming a security property it has not
/// got. Turning it off clears the element — including any hash that came from
/// the file, which is the honest reading of "unprotect".
#[wasm_bindgen]
pub fn session_set_sheet_protected(index: usize, on: bool) -> Result<(), JsError> {
    edit_sheet_metadata(index, move |_, data| {
        data.protection = on.then(casual_calc_model::SheetProtection::enabled);
    })
}

/// Each sheet's visibility as JSON `["visible"|"hidden"|"veryHidden", …]`, so
/// the host can leave hidden tabs out of the strip while still offering them in
/// an unhide list.
#[wasm_bindgen]
pub fn session_sheet_visibility() -> String {
    with_session(|s| {
        let items: Vec<String> = s
            .workbook()
            .sheets
            .iter()
            .map(|sheet| json_string(sheet.visibility.ooxml().unwrap_or("visible")))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Show or hide a sheet's tab. Hiding the last visible sheet is refused — a
/// workbook with nothing on screen has no way back.
#[wasm_bindgen]
pub fn session_set_sheet_visibility(index: usize, state: &str) -> Result<(), JsError> {
    {
        let next = SheetVisibility::from_ooxml(state);
        // The "at least one visible sheet" check reads the whole workbook and
        // can refuse, so it runs *before* the edit rather than inside it: an
        // operation closure has nowhere to report an error to.
        if !next.is_visible() {
            let visible = with_session(|s| {
                s.workbook()
                    .sheets
                    .iter()
                    .enumerate()
                    .filter(|(i, sh)| *i != index && sh.visibility.is_visible())
                    .count()
            })
            .unwrap_or(0);
            if visible == 0 {
                return Err(JsError::new("a workbook needs at least one visible sheet"));
            }
        }
        edit_sheet_metadata(index, move |_, data| {
            data.visibility = next;
        })
    }
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
                    // `is_row_hidden`, not `hidden_rows`: a filtered-out row has
                    // to collapse here too or the filter changes nothing on screen.
                    sh.is_row_hidden(line)
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

/// Which of `count` rows starting at `first` carry an explicit height, as a
/// JSON array of 0/1. A row the workbook sized itself is *pinned*: the editor's
/// auto row-height must not grow it, or an imported `ht="7.5"` would silently
/// become the editor's own idea of how tall the row should be (Excel likewise
/// stops auto-fitting a row once its height is set).
#[wasm_bindgen]
pub fn session_row_pinned(sheet: usize, first: u32, count: u32) -> String {
    with_session(|s| {
        let sizes = s.workbook().sheets.get(sheet).map(|sh| &sh.rows.sizes);
        let mut out = String::from("[");
        for i in 0..count {
            if i > 0 {
                out.push(',');
            }
            let pinned = sizes.is_some_and(|m| m.contains_key(&(first + i)));
            out.push(if pinned { '1' } else { '0' });
        }
        out.push(']');
        out
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Ceiling on how many auto-height candidates are reported. Beyond this the
/// host is told the list is incomplete rather than being handed a silent
/// truncation it would mistake for "no more rows grow".
const MAX_AUTOFIT_CANDIDATES: usize = 20_000;

/// Every cell that could make its row taller than the engine's height — wrapped
/// text, rotated text, or a font larger than the default — as JSON
/// `{"truncated":0|1,"cells":[{r,c,t,w,rot,fs,b,i,fn}]}`.
///
/// Row height for these can only be settled by measuring text, which is the
/// host's job. But if the host measures only the rows it can see, every offset
/// past the first grown row is wrong — scroll anchoring, the scrollbar extent
/// and scroll-into-view all read engine offsets that know nothing about the
/// growth. So this reports the candidates across the *whole* sheet, letting the
/// host build a complete picture once instead of a partial one per frame.
///
/// Rows the workbook sized itself are excluded: an explicit height wins over
/// auto-fit, exactly as in Excel, so those rows cannot grow.
#[wasm_bindgen]
pub fn session_autofit_candidates(sheet: usize) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return "{\"truncated\":0,\"default\":20,\"cells\":[]}".to_owned();
        };
        let mut cells = Vec::new();
        let mut truncated = false;
        for (at, cell) in sh.cells.iter() {
            if sh.rows.sizes.contains_key(&at.row) || sh.is_row_hidden(at.row) {
                continue;
            }
            let Some(style) = cell.style.and_then(|id| wb.styles.get(id)) else {
                continue;
            };
            let grows = style.wrap || style.rotation != 0 || style.font_size_hp.is_some();
            if !grows {
                continue;
            }
            let text = casual_calc_layout::display_text(wb, cell);
            if text.is_empty() {
                continue;
            }
            if cells.len() >= MAX_AUTOFIT_CANDIDATES {
                truncated = true;
                break;
            }
            let mut parts = vec![
                format!("\"r\":{}", at.row),
                format!("\"c\":{}", at.col),
                format!("\"t\":{}", json_string(&text)),
            ];
            if style.wrap {
                parts.push("\"w\":1".to_owned());
            }
            if style.rotation != 0 {
                parts.push(format!("\"rot\":{}", style.rotation));
            }
            if let Some(hp) = style.font_size_hp {
                parts.push(format!("\"fs\":{}", hp as f64 / 2.0));
            }
            if style.bold {
                parts.push("\"b\":1".to_owned());
            }
            if style.italic {
                parts.push("\"i\":1".to_owned());
            }
            if let Some(name) = &style.font_name {
                parts.push(format!("\"fn\":{}", json_string(name)));
            }
            cells.push(format!("{{{}}}", parts.join(",")));
        }
        // Candidates are never pinned, so they all sit at the sheet default —
        // reporting it once saves the host a call per row to discover that.
        let default_px = sh.rows.default.unwrap_or(DEFAULT_ROW_HEIGHT) * 96 / 1440;
        format!(
            "{{\"truncated\":{},\"default\":{},\"cells\":[{}]}}",
            u8::from(truncated),
            default_px,
            cells.join(",")
        )
    })
    .unwrap_or_else(|| "{\"truncated\":0,\"default\":20,\"cells\":[]}".to_owned())
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
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let columns = &mut data.columns;
            columns.default = Some(resize_px_to_twips(px));
            columns.sizes.clear();
        }
        session.edit(op).map_err(js)
    })
}

/// Set every row's height to `px` (the sheet default, clearing overrides).
#[wasm_bindgen]
pub fn session_set_all_row_height(sheet: usize, px: u32) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            let rows = &mut data.rows;
            rows.default = Some(resize_px_to_twips(px));
            rows.sizes.clear();
        }
        session.edit(op).map_err(js)
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
        // One pass per rule that needs the whole range — its extremes, its mean,
        // its top-N cutoff, how often each value repeats. Done once per call
        // rather than per cell: a colour scale over a thousand rows would
        // otherwise cost a thousand scans.
        let cf_stats: Vec<CfStats> = sheet
            .conditional_formats
            .iter()
            .map(|cf| cf_range_stats(wb, sheet, cf))
            .collect();

        let mut items = Vec::new();
        for (at, cell) in sheet.cells.row_band(first_row, last_row) {
            if at.col < first_col || at.col > last_col {
                continue;
            }
            let text = display_text(wb, cell);
            let style = cell.style.and_then(|id| wb.styles.get(id));
            // Conditional formatting overrides the cell's own fill when a rule
            // matches (first match wins). Numeric rules test the cell's number;
            // text rules test its display text.
            // Range-relative rules (colour scale, data bar) need where this
            // value sits between the range's own minimum and maximum, so they
            // are resolved against the pre-computed span rather than by a
            // per-cell predicate.
            let mut bar: Option<(f64, String)> = None;
            let mut cf_font: Option<String> = None;
            let mut cf_bold = false;
            // Lowest priority wins, and a matching `stopIfTrue` ends the search
            // — so rules are considered in priority order, not document order.
            let mut order: Vec<usize> = (0..sheet.conditional_formats.len()).collect();
            order.sort_by_key(|&i| {
                let p = sheet.conditional_formats[i].priority;
                (if p == 0 { u32::MAX } else { p }, i)
            });
            let mut cf_fill: Option<String> = None;
            for i in order {
                let cf = &sheet.conditional_formats[i];
                if !cf.covers(at.row, at.col) {
                    continue;
                }
                if cf.rule.has_own_presentation() {
                    let CellValue::Number(n) = cell.value else {
                        continue;
                    };
                    let (lo, hi) = (cf_stats[i].min, cf_stats[i].max);
                    // A flat range has no gradient to speak of; put everything at
                    // the top rather than dividing by zero.
                    let t = if hi > lo { (n - lo) / (hi - lo) } else { 1.0 };
                    match &cf.rule {
                        CfRule::ColorScale(colors) => {
                            cf_fill.get_or_insert_with(|| scale_color(colors, t));
                        }
                        CfRule::DataBar(color) => {
                            bar.get_or_insert((t.clamp(0.0, 1.0), color.clone()));
                        }
                        _ => {}
                    }
                    continue;
                }
                let hit = if cf.rule.needs_range_stats() {
                    cf_stats[i].matches(&cf.rule, &cell.value, &text)
                } else {
                    match cell.value {
                        CellValue::Number(n) => cf.rule.matches_number(n),
                        _ => cf.rule.matches_text(&text),
                    }
                };
                if !hit {
                    continue;
                }
                if !cf.fill.is_empty() {
                    cf_fill.get_or_insert_with(|| cf.fill.clone());
                }
                if let Some(fc) = &cf.font_color {
                    cf_font.get_or_insert_with(|| fc.clone());
                }
                cf_bold |= cf.bold;
                if cf.stop_if_true {
                    break;
                }
            }
            let fill = cf_fill
                .or_else(|| style.and_then(|s| s.fill_color.clone()))
                .unwrap_or_default();
            let has_border = style.is_some_and(|s| s.border.is_some());
            // A `centerContinuous` cell earns its place in the payload even when
            // it is otherwise bare: the run of such cells is exactly what the
            // label is centred across, so the host has to be able to see them.
            let center_across = style
                .and_then(|s| s.align)
                .is_some_and(|a| a == HAlign::CenterContinuous);
            if text.is_empty() && fill.is_empty() && !has_border && !center_across {
                continue;
            }
            // Explicit alignment wins; otherwise numbers/bools/errors go right.
            // The modes that are more than an edge (fill, justify, centre-across,
            // distributed) travel under their own token so the host can lay them
            // out rather than guessing an edge from them.
            let align = match style.and_then(|s| s.align) {
                Some(HAlign::Left) => "l",
                Some(HAlign::Center) => "c",
                Some(HAlign::Right) => "r",
                Some(HAlign::Fill) => "fill",
                Some(HAlign::Justify) => "just",
                Some(HAlign::CenterContinuous) => "cont",
                Some(HAlign::Distributed) => "dist",
                None => match cell.value {
                    CellValue::Number(_) | CellValue::Bool(_) | CellValue::Error(_) => "r",
                    _ => "l",
                },
            };
            let mut extra = String::new();
            // Numeric-valued cells are flagged so the host can apply Excel's
            // rule that a number too wide for its column renders as "#######"
            // rather than spilling or — worse — being clipped into a shorter
            // number that still reads as a plausible value.
            if matches!(cell.value, CellValue::Number(_)) {
                extra.push_str(",\"n\":1");
            }
            // Flagged rather than inferred from the text: a *text* cell may
            // legitimately contain "#VALUE!", and marking that as an error
            // would be a lie about the user's data.
            if matches!(cell.value, CellValue::Error(_)) {
                extra.push_str(",\"er\":1");
            }
            if let Some((frac, color)) = &bar {
                extra.push_str(&format!(
                    ",\"bar\":{:.4},\"barc\":{}",
                    frac,
                    json_string(color)
                ));
            }
            if cf_bold || style.is_some_and(|s| s.bold) {
                extra.push_str(",\"b\":1");
            }
            if style.is_some_and(|s| s.italic) {
                extra.push_str(",\"i\":1");
            }
            if let Some(u) = style.and_then(|s| s.underline) {
                extra.push_str(",\"u\":1");
                // The kind travels beside the flag rather than replacing it, so
                // every existing reader of `u` keeps working while a renderer
                // that cares can draw a double or accounting rule.
                extra.push_str(&format!(",\"uk\":{}", json_string(u.ooxml())));
            }
            // Superscript / subscript on the *cell* font. `va` is already taken
            // by vertical alignment, which is a different property with a
            // confusingly similar name in the format.
            if let Some(v) = style.and_then(|s| s.vert_align) {
                extra.push_str(&format!(",\"sup\":{}", json_string(v.ooxml())));
            }
            if style.is_some_and(|s| s.wrap) {
                extra.push_str(",\"w\":1");
            }
            if style.is_some_and(|s| s.clip) {
                extra.push_str(",\"cl\":1");
            }
            if let Some(ind) = style.map(|s| s.indent).filter(|i| *i > 0) {
                extra.push_str(&format!(",\"in\":{ind}"));
            }
            if let Some(rot) = style.map(|s| s.rotation).filter(|r| *r > 0) {
                extra.push_str(&format!(",\"rot\":{rot}"));
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
                    VAlign::Justify => "vj",
                    VAlign::Distributed => "vd",
                };
                extra.push_str(&format!(",\"va\":\"{t}\""));
            }
            // A number format may name the colour of its own output
            // (`#,##0;[Red]-#,##0`); that is a deliberate instruction about
            // this value, so it wins over the style's font colour, as in Excel.
            // A conditional format's font colour outranks the cell's own — the
            // rule is a statement about *this* value.
            let fc = cf_font
                .as_deref()
                .or_else(|| display_color(wb, cell))
                .or_else(|| style.and_then(|s| s.font_color.as_deref()));
            if let Some(fc) = fc {
                extra.push_str(&format!(",\"fc\":{}", json_string(fc)));
            }
            if !fill.is_empty() {
                extra.push_str(&format!(",\"bg\":{}", json_string(&fill)));
            }
            if let Some(bd) = style.and_then(|s| s.border.as_ref()) {
                extra.push_str(&format!(",\"bd\":{}", border_json(bd)));
            }
            // Fill detail beyond a flat colour: a pattern's second colour and
            // a gradient's stops. Sent only when present — nearly every filled
            // cell is a plain solid.
            if let Some(st) = style {
                if let Some(bg2) = &st.fill_bg_color {
                    extra.push_str(&format!(",\"bg2\":{}", json_string(bg2)));
                }
                if let Some(p) = &st.fill_pattern {
                    extra.push_str(&format!(",\"pat\":{}", json_string(p)));
                }
                if let Some(g) = &st.fill_gradient {
                    let stops: Vec<String> = g
                        .stops
                        .iter()
                        .map(|s| {
                            format!(
                                "{{\"p\":{:.4},\"c\":{}}}",
                                casual_calc_model::from_micro(s.position_micro),
                                json_string(&s.color)
                            )
                        })
                        .collect();
                    extra.push_str(&format!(
                        ",\"grad\":{{\"deg\":{:.2},\"stops\":[{}]}}",
                        casual_calc_model::from_micro(g.degree_micro),
                        stops.join(",")
                    ));
                }
                if st.shrink_to_fit {
                    extra.push_str(",\"shrink\":1");
                }
                if st.quote_prefix {
                    extra.push_str(",\"qp\":1");
                }
            }

            // Rich text: the per-run formatting, so the canvas can draw a cell
            // whose parts differ. Emitted only when the string actually has
            // runs — the overwhelming majority do not, and a `runs` key on
            // every cell would bloat a screenful of payload for nothing.
            if let CellValue::SharedString(id) | CellValue::InlineString(id) = cell.value
                && let Some(runs) = wb.strings.runs(id)
            {
                let parts: Vec<String> = runs
                    .iter()
                    .map(|run| {
                        let mut f = String::new();
                        if let Some(font) = &run.font {
                            if font.bold {
                                f.push_str(",\"b\":1");
                            }
                            if font.italic {
                                f.push_str(",\"i\":1");
                            }
                            if font.strike {
                                f.push_str(",\"st\":1");
                            }
                            if font.underline.is_some() {
                                f.push_str(",\"u\":1");
                            }
                            if let Some(v) = font.vert_align {
                                f.push_str(&format!(",\"va\":\"{}\"", v.ooxml()));
                            }
                            if let Some(c) = &font.color {
                                f.push_str(&format!(",\"fc\":{}", json_string(c)));
                            }
                            if let Some(sz) = font.size_hp {
                                f.push_str(&format!(",\"fs\":{}", sz as f64 / 2.0));
                            }
                            if let Some(name) = &font.name {
                                f.push_str(&format!(",\"fn\":{}", json_string(name)));
                            }
                        }
                        format!("{{\"t\":{}{f}}}", json_string(&run.text))
                    })
                    .collect();
                extra.push_str(&format!(",\"runs\":[{}]", parts.join(",")));
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
    // Single source of truth: the eval crate's dispatch catalog.
    let items: Vec<String> = casual_calc_eval::FUNCTIONS
        .iter()
        .map(|(n, sig)| format!("{{\"n\":{},\"sig\":{}}}", json_string(n), json_string(sig)))
        .collect();
    format!("[{}]", items.join(","))
}

/// The colour at position `t` (0..1) along a 2- or 3-stop scale, as `RRGGBB`.
/// Interpolated in plain RGB, which is what Excel does for colour scales.
fn scale_color(colors: &[String], t: f64) -> String {
    let parse = |hex: &str| -> (f64, f64, f64) {
        let v = u32::from_str_radix(hex, 16).unwrap_or(0);
        (
            f64::from((v >> 16) & 0xff),
            f64::from((v >> 8) & 0xff),
            f64::from(v & 0xff),
        )
    };
    if colors.is_empty() {
        return String::new();
    }
    let t = t.clamp(0.0, 1.0);
    // With three stops the midpoint is its own anchor, so each half interpolates
    // separately — otherwise the middle colour would never appear.
    let (a, b, local) = if colors.len() >= 3 {
        if t < 0.5 {
            (&colors[0], &colors[1], t * 2.0)
        } else {
            (&colors[1], &colors[2], (t - 0.5) * 2.0)
        }
    } else {
        (&colors[0], &colors[colors.len() - 1], t)
    };
    let (ar, ag, ab) = parse(a);
    let (br, bg, bb) = parse(b);
    let mix = |x: f64, y: f64| (x + (y - x) * local).round() as u32;
    format!("{:02X}{:02X}{:02X}", mix(ar, br), mix(ag, bg), mix(ab, bb))
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

/// A range as `A1:B2`, for display.
fn range_a1(range: &CellRange) -> String {
    let cell = |c: CellRef| {
        format!(
            "{}{}",
            casual_calc_formula::column_to_letters(c.col),
            c.row + 1
        )
    };
    if range.start == range.end {
        cell(range.start)
    } else {
        format!("{}:{}", cell(range.start), cell(range.end))
    }
}

/// A one-line human description of a rule, for the Manage Rules list.
fn describe_cf_rule(rule: &CfRule) -> String {
    match rule {
        CfRule::GreaterThan(x) => format!("greater than {x}"),
        CfRule::LessThan(x) => format!("less than {x}"),
        CfRule::EqualTo(x) => format!("equal to {x}"),
        CfRule::Between(a, b) => format!("between {a} and {b}"),
        CfRule::TextContains(t) => format!("text contains \"{t}\""),
        CfRule::ColorScale(c) => format!("colour scale ({} stops)", c.len()),
        CfRule::DataBar(_) => "data bar".to_owned(),
        CfRule::Top10 {
            rank,
            bottom,
            percent,
        } => format!(
            "{} {rank}{}",
            if *bottom { "bottom" } else { "top" },
            if *percent { "%" } else { "" }
        ),
        CfRule::AboveAverage { below, equal } => format!(
            "{} average{}",
            if *below { "below" } else { "above" },
            if *equal { " (or equal)" } else { "" }
        ),
        CfRule::DuplicateValues { unique } => if *unique {
            "appears only once"
        } else {
            "duplicated"
        }
        .to_owned(),
    }
}

/// Delete the rule at document index `index`.
#[wasm_bindgen]
pub fn session_delete_cf_rule(sheet: usize, index: usize) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if index < data.conditional_formats.len() {
            data.conditional_formats.remove(index);
        }
    })
}

/// Move the rule at `index` earlier (`up`) or later in evaluation order.
///
/// Rewrites every rule's priority to a dense 1..n afterwards, so the order is
/// unambiguous rather than depending on ties broken by document position.
#[wasm_bindgen]
pub fn session_reorder_cf_rule(sheet: usize, index: usize, up: bool) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        let n = data.conditional_formats.len();
        if index >= n {
            return;
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| {
            let p = data.conditional_formats[i].priority;
            (if p == 0 { u32::MAX } else { p }, i)
        });
        let Some(pos) = order.iter().position(|&i| i == index) else {
            return;
        };
        let swap_with = if up {
            if pos == 0 {
                return;
            }
            pos - 1
        } else {
            if pos + 1 >= n {
                return;
            }
            pos + 1
        };
        order.swap(pos, swap_with);
        for (rank, &i) in order.iter().enumerate() {
            data.conditional_formats[i].priority = rank as u32 + 1;
        }
    })
}

/// Turn `stopIfTrue` on or off for the rule at `index`.
#[wasm_bindgen]
pub fn session_set_cf_stop(sheet: usize, index: usize, stop: bool) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(cf) = data.conditional_formats.get_mut(index) {
            cf.stop_if_true = stop;
        }
    })
}

/// The cells a formula reads (`deps=false`) or the formulas that read this cell
/// (`deps=true`), as JSON `[{s,r0,c0,r1,c1}]` — blocks, since a range precedent
/// is one arrow, not one per cell.
///
/// Precedents come from the same walk the recalculator uses for its dirty set,
/// so a traced arrow can never point somewhere recalculation would not follow.
#[wasm_bindgen]
pub fn session_trace(sheet: usize, row: u32, col: u32, deps: bool) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let at = CellRef::new(row, col);
        let blocks: Vec<(usize, u32, u32, u32, u32)> = if deps {
            casual_calc_eval::dependents_of(wb, sheet, at)
                .into_iter()
                .map(|(si, r, c)| (si, r, c, r, c))
                .collect()
        } else {
            casual_calc_eval::precedents_of(wb, sheet, at)
        };
        let items: Vec<String> = blocks
            .iter()
            .map(|(si, r0, c0, r1, c1)| {
                format!("{{\"s\":{si},\"r0\":{r0},\"c0\":{c0},\"r1\":{r1},\"c1\":{c1}}}")
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Insert or delete a *block* of cells, shifting the rest along one axis.
///
/// Unlike a whole row or column insert, this does **not** rewrite formula
/// references: a reference into the shifted band would need the same
/// range-aware rewriting the structural ops do, and doing it half-way would be
/// worse than not doing it. `session_cells_shift_affects_formulas` lets the host
/// warn first, so the user decides rather than finding out later.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_shift_cells(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    insert: bool,
    vertical: bool,
) -> Result<(), JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    let span = if vertical {
        rr1 - rr0 + 1
    } else {
        cc1 - cc0 + 1
    };
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet) else {
            return Ok(());
        };
        // The cells that move, read before anything is written.
        let mut moving: Vec<(CellRef, Cell)> = Vec::new();
        for (at, c) in sh.cells.iter() {
            let in_band = if vertical {
                at.col >= cc0 && at.col <= cc1 && at.row >= rr0
            } else {
                at.row >= rr0 && at.row <= rr1 && at.col >= cc0
            };
            if in_band {
                moving.push((at, c.clone()));
            }
        }
        // Deleting drops the block itself; inserting keeps everything and pushes
        // it along.
        let mut ops: Vec<EditOperation> = Vec::new();
        for (at, _) in &moving {
            ops.push(EditOperation::ClearCell { sheet, at: *at });
        }
        for (at, c) in moving {
            let moved = if insert {
                if vertical {
                    Some(CellRef::new(at.row + span, at.col))
                } else {
                    Some(CellRef::new(at.row, at.col + span))
                }
            } else if vertical {
                if at.row <= rr1 {
                    None // inside the deleted block
                } else {
                    Some(CellRef::new(at.row - span, at.col))
                }
            } else if at.col <= cc1 {
                None
            } else {
                Some(CellRef::new(at.row, at.col - span))
            };
            if let Some(to) = moved {
                ops.push(EditOperation::SetCell {
                    sheet,
                    at: to,
                    cell: Some(c),
                });
            }
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Whether any formula reads a cell that a shift of this block would move — the
/// question the host has to ask before offering it, since references are not
/// rewritten.
#[wasm_bindgen]
pub fn session_shift_affects_formulas(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    vertical: bool,
) -> bool {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return false;
        };
        let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
        // Any populated cell in the band that something reads.
        sh.cells.iter().any(|(at, _)| {
            let in_band = if vertical {
                at.col >= cc0 && at.col <= cc1 && at.row >= rr0
            } else {
                at.row >= rr0 && at.row <= rr1 && at.col >= cc0
            };
            in_band && !casual_calc_eval::dependents_of(wb, sheet, at).is_empty()
        })
    })
    .unwrap_or(false)
}

/// A one-line summary of what the importer could not fully represent, or empty
/// when the file came through clean.
///
/// The report has existed since the importer did; nothing surfaced it, so a
/// lossy import looked identical to a faithful one. Anything not fully mapped is
/// something the user is about to save over, and they should hear it now rather
/// than discover it when the file reopens elsewhere.
#[wasm_bindgen]
pub fn session_import_summary() -> String {
    with_session(|s| {
        let report = s.compatibility_report();
        if report.is_clean() {
            return String::new();
        }
        let mut dropped: Vec<String> = Vec::new();
        let mut degraded: Vec<String> = Vec::new();
        for e in report.entries() {
            match e.model {
                casual_calc_import::ModelOutcome::Omitted => dropped.push(e.feature),
                casual_calc_import::ModelOutcome::Degraded => degraded.push(e.feature),
                casual_calc_import::ModelOutcome::Mapped => {}
            }
        }
        let mut parts = Vec::new();
        // Named, not counted: "3 features degraded" tells you nothing you can act
        // on, while "f" tells you to go and look at your formulas.
        if !dropped.is_empty() {
            parts.push(format!("not read: {}", dropped.join(", ")));
        }
        if !degraded.is_empty() {
            parts.push(format!("partly read: {}", degraded.join(", ")));
        }
        parts.join("; ")
    })
    .unwrap_or_default()
}

/// Whole-range statistics for the conditional-format rules that cannot be
/// decided from a cell alone. Computed once per rule per `session_cells` call.
#[derive(Default)]
struct CfStats {
    /// Smallest numeric value in the range (`INFINITY` when there are none).
    min: f64,
    /// Largest numeric value.
    max: f64,
    /// Mean of the numeric values.
    mean: f64,
    /// The top-N cutoff for a `Top10` rule: a value passes when it is at least
    /// this (or at most, for `bottom`). Precomputed so the per-cell test stays
    /// a comparison rather than a re-sort.
    cutoff: f64,
    /// How many times each display value occurs, for duplicate/unique rules.
    /// Empty unless such a rule needs it — building it for every rule would
    /// allocate a string per cell for nothing.
    counts: HashMap<String, u32>,
}

impl CfStats {
    /// Whether a cell passes a rule that needed these statistics.
    fn matches(&self, rule: &CfRule, value: &CellValue, text: &str) -> bool {
        match rule {
            CfRule::Top10 { bottom, .. } => {
                let CellValue::Number(n) = value else {
                    return false;
                };
                if *bottom {
                    *n <= self.cutoff
                } else {
                    *n >= self.cutoff
                }
            }
            CfRule::AboveAverage { below, equal } => {
                let CellValue::Number(n) = value else {
                    return false;
                };
                // Compare against the mean with an epsilon so a value that is
                // arithmetically equal does not fall on the wrong side of it.
                let d = n - self.mean;
                if d.abs() < 1e-9 {
                    return *equal;
                }
                if *below { d < 0.0 } else { d > 0.0 }
            }
            CfRule::DuplicateValues { unique } => {
                // A blank is neither duplicated nor unique — Excel skips them.
                if text.is_empty() {
                    return false;
                }
                let n = self.counts.get(text).copied().unwrap_or(0);
                if *unique { n == 1 } else { n > 1 }
            }
            _ => false,
        }
    }
}

/// Compute the statistics a rule needs, or defaults when it needs none.
fn cf_range_stats(wb: &Workbook, sheet: &Sheet, cf: &ConditionalFormat) -> CfStats {
    let mut stats = CfStats {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        ..Default::default()
    };
    if !cf.rule.needs_range_stats() {
        return stats;
    }
    let wants_counts = matches!(cf.rule, CfRule::DuplicateValues { .. });
    let mut values: Vec<f64> = Vec::new();
    for r in cf.range.start.row..=cf.range.end.row {
        for c in cf.range.start.col..=cf.range.end.col {
            let Some(cell) = sheet.cells.get(CellRef::new(r, c)) else {
                continue;
            };
            if let CellValue::Number(n) = cell.value
                && n.is_finite()
            {
                stats.min = stats.min.min(n);
                stats.max = stats.max.max(n);
                values.push(n);
            }
            if wants_counts {
                // Duplicate/unique compare what is *displayed*, so two cells
                // showing "1.0" and "1" count as different — as they do in Excel.
                // Compare what is *displayed*: two cells showing the same thing
                // are duplicates even if one is text and one a number, which is
                // how Excel treats them.
                let key = casual_calc_layout::display_text(wb, cell);
                if !key.is_empty() {
                    *stats.counts.entry(key).or_insert(0) += 1;
                }
            }
        }
    }
    if !values.is_empty() {
        stats.mean = values.iter().sum::<f64>() / values.len() as f64;
    }
    if let CfRule::Top10 {
        rank,
        bottom,
        percent,
    } = &cf.rule
        && !values.is_empty()
    {
        // Sort once and index, rather than testing each cell against the rest.
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = values.len();
        let take = if *percent {
            // Excel rounds a percentage down but always keeps at least one.
            (((*rank as f64 / 100.0) * n as f64).floor() as usize).clamp(1, n)
        } else {
            (*rank as usize).clamp(1, n)
        };
        stats.cutoff = if *bottom {
            values[take - 1]
        } else {
            values[n - take]
        };
    }
    stats
}

// --- Named cell styles ----------------------------------------------------

/// Excel's stock gallery, with the `builtinId`s Excel keys its own gallery off
/// — the *name* is localized, the id is not, so a file written by a French Excel
/// still lines up. A workbook that already defines a style of the same name uses
/// its definition instead of this one.
fn builtin_cell_styles() -> Vec<(&'static str, u32, Style)> {
    let tinted = |fill: &str, font: &str| Style {
        fill_color: Some(fill.to_owned()),
        font_color: Some(font.to_owned()),
        ..Default::default()
    };
    let heading = |size_hp: u32| Style {
        bold: true,
        font_size_hp: Some(size_hp),
        font_color: Some("1F4E79".to_owned()),
        ..Default::default()
    };
    vec![
        ("Normal", 0, Style::default()),
        ("Good", 26, tinted("C6EFCE", "006100")),
        ("Bad", 27, tinted("FFC7CE", "9C0006")),
        ("Neutral", 28, tinted("FFEB9C", "9C6500")),
        ("Title", 15, heading(36)),
        ("Heading 1", 16, heading(30)),
        ("Heading 2", 17, heading(26)),
        ("Heading 3", 18, heading(22)),
        ("Heading 4", 19, heading(22)),
        (
            "Total",
            25,
            Style {
                bold: true,
                ..Default::default()
            },
        ),
    ]
}

/// The cell styles to offer in a gallery, as JSON
/// `[{n,b,bold,fg,bg,sz}]` — name, builtin id, and enough formatting for the
/// host to preview each entry in its own look.
///
/// The workbook's own styles come first; the stock gallery fills in the rest, so
/// a file that defines "Heading 1" shows *its* Heading 1.
#[wasm_bindgen]
pub fn session_cell_styles() -> String {
    let mut out: Vec<(String, Option<u32>, Style)> = with_session(|s| {
        s.workbook()
            .cell_styles
            .iter()
            .map(|c| (c.name.clone(), c.builtin_id, c.style.clone()))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for (name, builtin, style) in builtin_cell_styles() {
        if !out.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(name)) {
            out.push((name.to_owned(), Some(builtin), style));
        }
    }
    let items: Vec<String> = out
        .iter()
        .map(|(name, builtin, st)| {
            let mut parts = vec![format!("\"n\":{}", json_string(name))];
            if let Some(b) = builtin {
                parts.push(format!("\"b\":{b}"));
            }
            if st.bold {
                parts.push("\"bold\":1".to_owned());
            }
            if let Some(c) = &st.font_color {
                parts.push(format!("\"fg\":{}", json_string(c)));
            }
            if let Some(c) = &st.fill_color {
                parts.push(format!("\"bg\":{}", json_string(c)));
            }
            if let Some(hp) = st.font_size_hp {
                parts.push(format!("\"sz\":{}", hp as f64 / 2.0));
            }
            format!("{{{}}}", parts.join(","))
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Apply the named cell style `name` across a range (one undo step).
///
/// The style's formatting is written onto each cell *and* the association is
/// recorded, so the cells still say which style they belong to after a save. An
/// unknown name is a no-op rather than an error — the gallery is the only caller
/// and it only offers names this returns.
#[wasm_bindgen]
pub fn session_apply_cell_style(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    name: &str,
) -> Result<(), JsError> {
    // Make sure the workbook actually defines the style, so the link has
    // something to point at and the name survives the save.
    let index = SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut()?;
        let wb = session.workbook_mut();
        if let Some(i) = wb
            .cell_styles
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
        {
            return Some((i as u32, wb.cell_styles[i].style.clone()));
        }
        let (n, b, style) = builtin_cell_styles()
            .into_iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(name))?;
        wb.cell_styles.push(casual_calc_model::NamedCellStyle {
            name: n.to_owned(),
            builtin_id: Some(b),
            style: style.clone(),
        });
        Some((wb.cell_styles.len() as u32 - 1, style))
    });
    let Some((index, style)) = index else {
        return Ok(());
    };
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        // Replace rather than merge: picking a named style means "look like
        // this", and a leftover fill from the previous style would make it
        // look like neither.
        let mut next = style.clone();
        next.number_format = st.number_format.clone();
        next.style_ref = Some(index);
        *st = next;
    })
}

/// The workbook's theme colours as a JSON array of `RRGGBB`, in OOXML slot
/// order, or the stock Office scheme when the package carried no theme part.
///
/// A colour picker that offers "theme colours" has to offer *this file's*
/// theme; the stock ten would be a plausible-looking lie about a workbook that
/// uses its own scheme.
#[wasm_bindgen]
pub fn theme_colors() -> String {
    let items: Vec<String> = with_session(|s| {
        let wb = s.workbook();
        if wb.theme_colors.is_empty() {
            None
        } else {
            Some(wb.theme_colors.iter().map(|c| json_string(c)).collect())
        }
    })
    .flatten()
    .unwrap_or_else(|| {
        casual_calc_import::stock_theme_slots()
            .iter()
            .map(|c| json_string(c))
            .collect()
    });
    format!("[{}]", items.join(","))
}

/// The font families to offer in a host's font picker, as JSON
/// `[{n,f,k}, …]` — name, the bundled family it renders as, and the fidelity
/// of that match (`"exact"` / `"metric"` / `"generic"`). Sourced from the
/// shared substitution table so the picker can never offer a family this build
/// cannot render faithfully; the editor still accepts any typed name.
#[wasm_bindgen]
pub fn font_families() -> String {
    use casual_calc_layout::SubstituteKind;
    let items: Vec<String> = casual_calc_layout::PICKER_FAMILIES
        .iter()
        .filter_map(|name| {
            let sub = casual_calc_layout::substitute(name)?;
            let kind = match sub.kind {
                SubstituteKind::Bundled => "exact",
                SubstituteKind::MetricCompatible => "metric",
                SubstituteKind::Generic => "generic",
            };
            Some(format!(
                "{{\"n\":{},\"f\":{},\"k\":\"{kind}\"}}",
                json_string(name),
                json_string(sub.family.name)
            ))
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// The cell references inside formula text, as JSON
/// `[{s,e,r0,c0,r1,c1,sh?}, …]` — the character span of each reference plus the
/// block it covers, in the order they appear. Drives the editor's range finder
/// (colored outlines on the grid while a formula is being edited).
///
/// Shared with the parser rather than re-derived in the host: whether a name is
/// a reference or a function call, and what counts as inside a string literal,
/// must be the engine's answer.
#[wasm_bindgen]
pub fn formula_ref_spans(text: &str) -> String {
    let items: Vec<String> = casual_calc_formula::reference_spans(text)
        .into_iter()
        .map(|r| {
            let sheet = r
                .sheet
                .map(|s| format!(",\"sh\":{}", json_string(&s)))
                .unwrap_or_default();
            format!(
                "{{\"s\":{},\"e\":{},\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{}{sheet}}}",
                r.start, r.end, r.row0, r.col0, r.row1, r.col1
            )
        })
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
    // The toolbar toggle is binary, so it flips between "no underline" and the
    // plain single line. A cell already carrying a double or accounting
    // underline reads as underlined and toggles off, which is what Excel's own
    // button does — it does not cycle through the variants.
    let target = !range_all(sheet, r0, c0, r1, c1, |st| st.underline.is_some());
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.underline = target.then_some(Underline::Single)
    })
}

/// Set how a range's text behaves when it does not fit its column:
/// `"overflow"` (spill into empty neighbours — the default and what Excel
/// always does), `"wrap"`, or `"clip"` (stop at the cell edge).
///
/// These are one three-way choice, not two independent flags, which is why they
/// are set together: wrap and clip cannot both be on.
#[wasm_bindgen]
pub fn session_set_text_overflow(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    mode: &str,
) -> Result<(), JsError> {
    let (wrap, clip) = match mode {
        "wrap" => (true, false),
        "clip" => (false, true),
        _ => (false, false), // "overflow" — the default
    };
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.wrap = wrap;
        st.clip = clip;
    })
}

/// Toggle wrap on a range (the toolbar button). Prefer
/// [`session_set_text_overflow`] when setting an explicit mode.
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

/// The built-in text fill lists (month and weekday names, full and abbreviated).
/// Autofill extends a source drawn from one of these — `Jan, Feb → Mar` — and a
/// single name extends too (`Jan → Feb, Mar`), matching Excel.
const FILL_LISTS: &[&[&str]] = &[
    &[
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    &[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    &[
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ],
    &["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
];

/// Locate `text` (case-insensitively) in the fill lists → `(list, item index)`.
fn find_in_fill_lists(text: &str) -> Option<(usize, usize)> {
    let t = text.trim();
    for (li, list) in FILL_LISTS.iter().enumerate() {
        if let Some(ii) = list.iter().position(|w| w.eq_ignore_ascii_case(t)) {
            return Some((li, ii));
        }
    }
    None
}

/// Detect whether a source line of text values is a named-list sequence.
/// Returns `(list index, start item index, step)`; the step wraps modulo the
/// list length, so a descending drag (`Dec, Nov`) continues correctly. A single
/// recognized name yields step `+1` (Excel extends a lone month/weekday).
fn detect_text_series(vals: &[Option<String>]) -> Option<(usize, i64, i64)> {
    if vals.iter().any(|v| v.is_none()) {
        return None;
    }
    let mut idxs = Vec::with_capacity(vals.len());
    let mut list_id = None;
    for v in vals {
        let (li, ii) = find_in_fill_lists(v.as_ref().unwrap())?;
        match list_id {
            None => list_id = Some(li),
            Some(prev) if prev != li => return None, // mixed lists
            _ => {}
        }
        idxs.push(ii as i64);
    }
    let li = list_id?;
    let len = FILL_LISTS[li].len() as i64;
    if idxs.len() == 1 {
        return Some((li, idxs[0], 1));
    }
    let step = (idxs[1] - idxs[0]).rem_euclid(len);
    for w in idxs.windows(2) {
        if (w[1] - w[0]).rem_euclid(len) != step {
            return None;
        }
    }
    Some((li, idxs[0], step))
}

/// The name a text series produces at forward offset `k` from its start.
fn text_series_at(list_id: usize, idx0: i64, step: i64, k: i64) -> String {
    let list = FILL_LISTS[list_id];
    let len = list.len() as i64;
    list[(idx0 + step * k).rem_euclid(len) as usize].to_owned()
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
    session_fill_mode(sheet, sr0, sc0, sr1, sc1, dr0, dc0, dr1, dc1, "auto")
}

/// Fill with an explicit mode, for the fill-options popup and the Ctrl toggle.
///
/// - `auto` — detect a series, else tile (what dragging the handle does)
/// - `copy` — always tile, even where a series was detectable
/// - `series` — force a linear series, stepping by 1 from a single cell
/// - `growth` — geometric: continue by ratio rather than by difference
/// - `formats` — carry only the styling
/// - `values` — carry only the values, leaving the target's styling alone
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_fill_mode(
    sheet: usize,
    sr0: u32,
    sc0: u32,
    sr1: u32,
    sc1: u32,
    dr0: u32,
    dc0: u32,
    dr1: u32,
    dc1: u32,
    mode: &str,
) -> Result<(), JsError> {
    let (src_rows, src_cols) = ((sr1 - sr0 + 1) as i64, (sc1 - sc0 + 1) as i64);
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        // Pass 1 (immutable): resolve each destination cell's source + shifted formula.
        struct Pending {
            at: CellRef,
            value: CellValue,
            /// A named-list series result to intern into a string value in pass 2
            /// (interning needs `&mut workbook`, unavailable in the read pass).
            text: Option<String>,
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
            // A string literal (no formula) at (r,c), for named-list series.
            let text_lit = |r: u32, c: u32| -> Option<String> {
                sh.cells
                    .get(CellRef::new(r, c))
                    .filter(|cell| cell.formula.is_none())
                    .and_then(|cell| match cell.value {
                        CellValue::SharedString(id) | CellValue::InlineString(id) => {
                            wb.strings.get(id).map(str::to_owned)
                        }
                        _ => None,
                    })
            };
            // If the fill grows along exactly one axis and each line of the
            // source is a numeric arithmetic sequence (>=2 cells, constant
            // step), extend the sequence instead of tiling — Excel's autofill.
            let vertical = dc0 == sc0 && dc1 == sc1 && (dr1 > sr1 || dr0 < sr0);
            let horizontal = dr0 == sr0 && dr1 == sr1 && (dc1 > sc1 || dc0 < sc0);
            let growth = mode == "growth";
            let arithmetic = |vals: &[Option<f64>]| -> Option<(f64, f64)> {
                // Copy never extends, whatever the values look like.
                if mode == "copy" || mode == "formats" {
                    return None;
                }
                if vals.iter().any(|v| v.is_none()) {
                    return None;
                }
                if vals.is_empty() {
                    return None;
                }
                if growth {
                    // Geometric: a constant *ratio* rather than a constant
                    // difference. A single cell doubles, matching Excel's
                    // default growth step.
                    let first = vals[0].unwrap();
                    if first == 0.0 {
                        return None; // no ratio can be recovered from zero
                    }
                    if vals.len() == 1 {
                        return Some((first, 2.0));
                    }
                    let ratio = vals[1].unwrap() / first;
                    for w in vals.windows(2) {
                        let a = w[0].unwrap();
                        if a == 0.0 || (w[1].unwrap() / a - ratio).abs() > 1e-9 {
                            return None;
                        }
                    }
                    return Some((first, ratio));
                }
                if vals.len() < 2 {
                    // An explicit "fill series" steps by one from a single cell;
                    // auto-detection needs two to know the step.
                    return (mode == "series").then(|| (vals[0].unwrap(), 1.0));
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
            // Named-list (month/weekday) series, per line, alongside the numeric.
            let col_text: Vec<Option<(usize, i64, i64)>> = if vertical {
                (sc0..=sc1)
                    .map(|c| {
                        detect_text_series(&(sr0..=sr1).map(|r| text_lit(r, c)).collect::<Vec<_>>())
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let row_text: Vec<Option<(usize, i64, i64)>> = if horizontal {
                (sr0..=sr1)
                    .map(|r| {
                        detect_text_series(&(sc0..=sc1).map(|c| text_lit(r, c)).collect::<Vec<_>>())
                    })
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
                    // Growth multiplies by the ratio; a linear series adds the
                    // step. `n` is how far along the fill axis this cell sits.
                    let project = |v0: f64, step: f64, n: i64| {
                        if growth {
                            v0 * step.powi(n as i32)
                        } else {
                            v0 + step * n as f64
                        }
                    };
                    let series_value = if vertical {
                        col_series[(dc - sc0) as usize]
                            .map(|(v0, step)| project(v0, step, dr as i64 - sr0 as i64))
                    } else if horizontal {
                        row_series[(dr - sr0) as usize]
                            .map(|(v0, step)| project(v0, step, dc as i64 - sc0 as i64))
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
                            text: None,
                            style,
                            formula: None,
                        });
                        continue;
                    }
                    // Named-list (month/weekday) series along the fill axis.
                    let text_series = if vertical {
                        col_text[(dc - sc0) as usize]
                            .map(|(li, i0, st)| text_series_at(li, i0, st, dr as i64 - sr0 as i64))
                    } else if horizontal {
                        row_text[(dr - sr0) as usize]
                            .map(|(li, i0, st)| text_series_at(li, i0, st, dc as i64 - sc0 as i64))
                    } else {
                        None
                    };
                    if let Some(name) = text_series {
                        let style = sh
                            .cells
                            .get(CellRef::new(sr as u32, sc as u32))
                            .and_then(|c| c.style);
                        pending.push(Pending {
                            at,
                            value: CellValue::Empty,
                            text: Some(name),
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
                                text: None,
                                style: c.style,
                                formula,
                            });
                        }
                        None => pending.push(Pending {
                            at,
                            value: CellValue::Empty,
                            text: None,
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
            // A named-list series result becomes an interned string value here.
            let mut value = match p.text {
                Some(name) => CellValue::SharedString(session.workbook_mut().intern_string(&name)),
                None => p.value,
            };
            let mut style = p.style;
            let mut formula = p.formula;
            // "Formatting only" and "without formatting" are the same fill with
            // one half discarded — the target keeps whatever the other half was.
            if mode == "formats" {
                value = CellValue::Empty;
                formula = None;
            } else if mode == "values" {
                style = session
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|sh| sh.cells.get(p.at))
                    .and_then(|c| c.style);
            }
            // Formatting-only must not erase the value already there.
            if mode == "formats"
                && let Some(existing) = session
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|sh| sh.cells.get(p.at))
            {
                value = existing.value.clone();
                formula = None;
            }
            let cell = if value.is_empty() && style.is_none() && formula.is_none() {
                None
            } else {
                let mut c = Cell::value(value);
                c.style = style;
                if let Some(expr) = formula {
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
    session_sort_range_multi(
        sheet,
        r0,
        c0,
        r1,
        c1,
        vec![key_col],
        vec![u8::from(ascending)],
    )
}

/// Sort a range by up to several key columns, each with its own direction.
///
/// `key_cols[i]` is compared before `key_cols[i + 1]`, so later keys only break
/// ties in the earlier ones — the "then by" of a sort dialog. `ascending[i]` is
/// 0 or 1 for the matching key. Callers exclude a header row by passing `r0`
/// one below it; this deliberately knows nothing about headers, because whether
/// the first row is one is a question about the *sheet*, not the sort.
#[wasm_bindgen]
pub fn session_sort_range_multi(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    key_cols: Vec<u32>,
    ascending: Vec<u8>,
) -> Result<(), JsError> {
    if key_cols.is_empty() || r1 <= r0 {
        return Ok(());
    }
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
            /// One sort key per key column, in `key_cols` order.
            keys: Vec<(u8, f64, String)>,
            blank: bool,
            cells: Vec<Option<RowCell>>,
        }
        let mut rows: Vec<Row> = Vec::new();
        if let Some(sh) = session.workbook().sheets.get(sheet) {
            let wb = session.workbook();
            for r in r0..=r1 {
                // Excel's type order: numbers, then text, then errors, then
                // blanks — so a mixed column sorts predictably.
                let sort_key = |value: &CellValue| match value {
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
                let value_at = |col: u32| {
                    sh.cells
                        .get(CellRef::new(r, col))
                        .map(|c| c.value.clone())
                        .unwrap_or(CellValue::Empty)
                };
                let kv = value_at(key_cols[0]);
                let keys: Vec<(u8, f64, String)> =
                    key_cols.iter().map(|c| sort_key(&value_at(*c))).collect();
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
                    keys,
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
            // Each key decides only if the ones before it tied, and carries its
            // own direction — "A→Z by Region, then Z→A by Total".
            for (i, (ka, kb)) in a.keys.iter().zip(b.keys.iter()).enumerate() {
                let ord =
                    ka.0.cmp(&kb.0)
                        .then_with(|| ka.1.partial_cmp(&kb.1).unwrap_or(Ordering::Equal))
                        .then_with(|| ka.2.cmp(&kb.2));
                if ord != Ordering::Equal {
                    return if ascending.get(i).copied().unwrap_or(1) != 0 {
                        ord
                    } else {
                        ord.reverse()
                    };
                }
            }
            Ordering::Equal
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

/// Set the four font flags explicitly across a range (one undo step).
///
/// The toolbar toggles are relative — they flip whatever the range currently is
/// — which a dialog cannot use: it shows checkboxes with definite states and has
/// to be able to apply them as such.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_font_flags(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
) -> Result<(), JsError> {
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.bold = bold;
        st.italic = italic;
        st.underline = underline.then_some(Underline::Single);
        st.strike = strike;
    })
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

/// Toggle subscript or superscript across a range (one undo step).
///
/// `which` is `"superscript"` or `"subscript"`; anything else clears it. The
/// two are mutually exclusive in OOXML — one `vertAlign` per font — so setting
/// one replaces the other rather than stacking.
#[wasm_bindgen]
pub fn session_toggle_vert_align(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    which: &str,
) -> Result<(), JsError> {
    let want = VertAlign::from_ooxml(which);
    // Pressing the button on a range already carrying it turns it off, which is
    // what every other character toggle here does.
    let already = want.is_some() && range_all(sheet, r0, c0, r1, c1, |st| st.vert_align == want);
    let target = if already { None } else { want };
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.vert_align = target)
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
    edit_sheet_metadata(sheet, move |sh, data| {
        let set = if columns {
            &mut data.hidden_cols
        } else {
            &mut data.hidden_rows
        };
        for i in a..=b {
            if hide {
                set.insert(i);
            } else {
                set.remove(&i);
            }
        }
        if hide {
            return;
        }
        // An outline collapse hides through this same set, so revealing lines by
        // hand can un-collapse a group behind its own toggle's back. Any summary
        // whose band is no longer fully hidden is no longer collapsed — without
        // this the toggle keeps showing "+" over rows that are plainly visible.
        let hidden = if columns {
            &data.hidden_cols
        } else {
            &data.hidden_rows
        };
        let summaries: Vec<u32> = if columns {
            data.collapsed_cols.iter().copied().collect()
        } else {
            data.collapsed_rows.iter().copied().collect()
        };
        let stale: Vec<u32> = summaries
            .into_iter()
            .filter(|&summary| match sh.outline_band(summary, columns) {
                Some((start, end)) => (start..=end).any(|i| !hidden.contains(&i)),
                None => true,
            })
            .collect();
        let collapsed = if columns {
            &mut data.collapsed_cols
        } else {
            &mut data.collapsed_rows
        };
        for summary in stale {
            collapsed.remove(&summary);
        }
    })
}

// --- Outline (row/column grouping) ----------------------------------------
//
// The model already carried outline levels and collapsed flags — import and
// export round-trip them — but nothing could create or toggle a group. These
// route through `SetSheetMetadata` so a group, and the rows a collapse hid, undo
// as one step.

/// OOXML caps outline nesting at seven levels.
const MAX_OUTLINE_LEVEL: u8 = 7;

/// Apply `edit` to the sheet's metadata bundle and commit it as one undo step.
/// The closure also gets the sheet itself, for the reads (outline bands, cell
/// text) that decide what the edit should be.
///
/// The bundle covers validations, conditional formats, comments, visibility and
/// protection as well as the positional state. Those five used to be mutated
/// straight through `workbook_mut()`, which is worse than having no undo: the
/// button stays enabled and the history keeps filling, so Ctrl+Z after editing a
/// comment reversed the preceding *cell* edit instead — destroying work the user
/// never touched, somewhere they were not looking. Everything that changes sheet
/// state goes through here.
fn edit_sheet_metadata(
    sheet: usize,
    edit: impl FnOnce(&Sheet, &mut SheetMetadata),
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
            return Ok(());
        };
        let mut data = SheetMetadata::capture(&sh);
        edit(&sh, &mut data);
        session
            .edit(EditOperation::SetSheetMetadata {
                sheet,
                data: Box::new(data),
            })
            .map_err(js)
    })
}

/// Nest lines `a..=b` one level deeper (`columns` picks the axis).
#[wasm_bindgen]
pub fn session_group(sheet: usize, a: u32, b: u32, columns: bool) -> Result<(), JsError> {
    let (lo, hi) = (a.min(b), a.max(b));
    edit_sheet_metadata(sheet, move |_, data| {
        let levels = if columns {
            &mut data.col_outline_levels
        } else {
            &mut data.row_outline_levels
        };
        for i in lo..=hi {
            let next = levels.get(&i).copied().unwrap_or(0).saturating_add(1);
            levels.insert(i, next.min(MAX_OUTLINE_LEVEL));
        }
    })
}

/// Lift lines `a..=b` one level out, dropping them from the outline at zero.
#[wasm_bindgen]
pub fn session_ungroup(sheet: usize, a: u32, b: u32, columns: bool) -> Result<(), JsError> {
    let (lo, hi) = (a.min(b), a.max(b));
    edit_sheet_metadata(sheet, move |_, data| {
        let (levels, collapsed, hidden) = if columns {
            (
                &mut data.col_outline_levels,
                &mut data.collapsed_cols,
                &mut data.hidden_cols,
            )
        } else {
            (
                &mut data.row_outline_levels,
                &mut data.collapsed_rows,
                &mut data.hidden_rows,
            )
        };
        for i in lo..=hi {
            match levels.get(&i).copied().unwrap_or(0) {
                0 | 1 => {
                    // Out of the outline entirely: a line with no level cannot
                    // stay collapsed, and must not stay hidden by a group that no
                    // longer exists.
                    levels.remove(&i);
                    collapsed.remove(&i);
                    hidden.remove(&i);
                }
                n => {
                    levels.insert(i, n - 1);
                }
            }
        }
    })
}

/// Collapse or expand the group whose summary line is `index`.
#[wasm_bindgen]
pub fn session_toggle_outline(sheet: usize, index: u32, columns: bool) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |sh, data| {
        let Some((start, end)) = sh.outline_band(index, columns) else {
            return;
        };
        let (collapsed, hidden) = if columns {
            (&mut data.collapsed_cols, &mut data.hidden_cols)
        } else {
            (&mut data.collapsed_rows, &mut data.hidden_rows)
        };
        // A collapse hides through the ordinary hidden set, because that is what
        // OOXML writes — a collapsed detail row is just `hidden="1"`. The
        // `collapsed` flag on the summary line is what remembers that a group,
        // rather than a person, did the hiding.
        if collapsed.remove(&index) {
            for i in start..=end {
                hidden.remove(&i);
            }
        } else {
            collapsed.insert(index);
            for i in start..=end {
                hidden.insert(i);
            }
        }
    })
}

/// Show outline levels up to `level` and collapse everything deeper — the
/// numbered level buttons. `level` 0 collapses every group.
#[wasm_bindgen]
pub fn session_show_outline_level(sheet: usize, level: u8, columns: bool) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |sh, data| {
        let levels: Vec<(u32, u8)> = if columns {
            sh.col_outline_levels
                .iter()
                .map(|(&k, &v)| (k, v))
                .collect()
        } else {
            sh.row_outline_levels
                .iter()
                .map(|(&k, &v)| (k, v))
                .collect()
        };
        let (collapsed, hidden) = if columns {
            (&mut data.collapsed_cols, &mut data.hidden_cols)
        } else {
            (&mut data.collapsed_rows, &mut data.hidden_rows)
        };
        for (i, l) in &levels {
            if *l > level {
                hidden.insert(*i);
            } else {
                hidden.remove(i);
            }
        }
        // A summary reads as collapsed exactly when its own band just went
        // hidden, so the toggles agree with what is on screen.
        collapsed.clear();
        for (i, l) in &levels {
            if *l <= level
                && let Some((start, _)) = sh.outline_band(*i, columns)
                && levels.iter().any(|(j, jl)| *j == start && *jl > level)
            {
                collapsed.insert(*i);
            }
        }
    })
}

/// The outline for `count` lines from `first`, as JSON
/// `{"max":N,"lines":[{"l":level,"c":0|1,"b":0|1}, …]}` — nesting level, whether
/// that line's group is collapsed, and whether a group hangs off it at all (so
/// the host knows where to draw a toggle).
///
/// `max` is the deepest level on the sheet, which sizes the gutter. Asking for
/// `count` 0 returns just that, which is how the host decides whether to reserve
/// any gutter at all — so a sheet with no outline costs one cheap call a frame.
#[wasm_bindgen]
pub fn session_outline(sheet: usize, first: u32, count: u32, columns: bool) -> String {
    const NONE: &str = "{\"max\":0,\"lines\":[]}";
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return NONE.to_owned();
        };
        let (levels, collapsed) = if columns {
            (&sh.col_outline_levels, &sh.collapsed_cols)
        } else {
            (&sh.row_outline_levels, &sh.collapsed_rows)
        };
        let max = levels.values().copied().max().unwrap_or(0);
        if max == 0 {
            return NONE.to_owned();
        }
        let lines: Vec<String> = (first..first.saturating_add(count))
            .map(|i| {
                let l = levels.get(&i).copied().unwrap_or(0);
                let c = u8::from(collapsed.contains(&i));
                let b = u8::from(sh.outline_band(i, columns).is_some());
                format!("{{\"l\":{l},\"c\":{c},\"b\":{b}}}")
            })
            .collect();
        format!("{{\"max\":{max},\"lines\":[{}]}}", lines.join(","))
    })
    .unwrap_or_else(|| NONE.to_owned())
}

// --- Autofilter -----------------------------------------------------------
//
// The rules live in the model; evaluating them lives here, because a checklist
// matches on the text the user *sees* and formatting is only available at this
// layer.
//
// Re-evaluation is explicit — on a filter change and on load — not on every
// cell edit. That is Excel's behaviour (a filtered-out row you edit stays put
// until you re-apply) and it keeps a keystroke from costing a full range scan.

/// Most distinct values a checklist will return. Excel's own limit is 10,000;
/// past that the UI has to offer a condition instead of a list. The payload
/// reports truncation rather than silently returning a short list.
const MAX_FILTER_VALUES: usize = 10_000;

/// A cell's display text and numeric value, for filter matching.
fn filter_operands(wb: &Workbook, sheet: &Sheet, row: u32, col: u32) -> (String, Option<f64>) {
    match sheet.cells.get(CellRef::new(row, col)) {
        Some(cell) => {
            let num = match cell.value {
                CellValue::Number(n) => Some(n),
                _ => None,
            };
            (casual_calc_layout::display_text(wb, cell), num)
        }
        None => (String::new(), None),
    }
}

/// Whether `row` passes every column rule except `skip` (used to build a
/// checklist that reflects what the *other* columns have already narrowed to).
fn row_passes(
    wb: &Workbook,
    sheet: &Sheet,
    filter: &AutoFilter,
    row: u32,
    skip: Option<u32>,
) -> bool {
    filter.rules.iter().all(|(&off, rule)| {
        if Some(off) == skip {
            return true;
        }
        let col = filter.range.start.col.saturating_add(off);
        let (text, num) = filter_operands(wb, sheet, row, col);
        rule.matches(&text, num)
    })
}

/// Every filter on a sheet: its own, then each table's, in table order.
///
/// A table filters independently of the sheet it sits on, so anything that
/// asks "what is filtered here" has to look at both — reading only
/// `sheet.auto_filter` is what left a table's header buttons inert.
fn sheet_filters(sheet: &Sheet) -> impl Iterator<Item = (FilterSite, &AutoFilter)> {
    sheet
        .auto_filter
        .iter()
        .map(|f| (FilterSite::Sheet, f))
        .chain(
            sheet
                .tables
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.auto_filter.as_ref().map(|f| (FilterSite::Table(i), f))),
        )
}

/// Where a filter lives. The host knows the button it drew, not which structure
/// owns it, so every operation resolves the site from a column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FilterSite {
    /// The worksheet's own `<autoFilter>`.
    Sheet,
    /// The table at this index in `sheet.tables`.
    Table(usize),
}

/// The filter whose range covers `col`, and where it lives.
///
/// The sheet's own filter wins when both cover the column: it is the one the
/// toolbar button turned on, so it is the one the user just interacted with.
fn filter_at_col(sheet: &Sheet, col: u32) -> Option<(FilterSite, &AutoFilter)> {
    sheet_filters(sheet).find(|(_, f)| col >= f.range.start.col && col <= f.range.end.col)
}

/// The rows every filter on the sheet hides, recomputed from their rules.
fn recompute_filter_hidden(wb: &Workbook, sheet: &Sheet) -> BTreeSet<u32> {
    let mut hidden = BTreeSet::new();
    for (_, filter) in sheet_filters(sheet) {
        if !filter.is_active() {
            continue;
        }
        for row in filter.body_start()..=filter.range.end.row {
            if !row_passes(wb, sheet, filter, row, None) {
                hidden.insert(row);
            }
        }
    }
    hidden
}

/// Install `filter` on a sheet and hide the rows it excludes, as one undoable
/// edit. Passing `None` turns the filter off and releases every row it hid.
fn commit_filter(
    sheet: usize,
    site: FilterSite,
    filter: Option<AutoFilter>,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        let wb = session.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return Ok(());
        };
        // Evaluate against a sheet carrying the *new* rules, not the ones
        // currently installed. Every other filter on the sheet still applies,
        // so the probe keeps them — clearing one table's filter must not
        // release rows another filter hides.
        let mut probe = sh.clone();
        match site {
            FilterSite::Sheet => probe.auto_filter = filter.clone(),
            FilterSite::Table(i) => match probe.tables.get_mut(i) {
                Some(t) => t.auto_filter = filter.clone(),
                None => return Ok(()),
            },
        }
        let hidden = recompute_filter_hidden(wb, &probe);
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            match site {
                FilterSite::Sheet => data.auto_filter = filter,
                FilterSite::Table(i) => {
                    if let Some(t) = data.tables.get_mut(i) {
                        t.auto_filter = filter;
                    }
                }
            }
            data.filter_hidden = hidden;
        }
        session.edit(op).map_err(js)
    })
}

/// Read a sheet's own autofilter, or `None` if it has none.
fn sheet_filter(sheet: usize) -> Option<AutoFilter> {
    with_session(|s| s.workbook().sheets.get(sheet)?.auto_filter.clone()).flatten()
}

/// Read the filter covering `col` — the sheet's or a table's — with its site.
fn filter_for_col(sheet: usize, col: u32) -> Option<(FilterSite, AutoFilter)> {
    with_session(|s| {
        let sh = s.workbook().sheets.get(sheet)?;
        filter_at_col(sh, col).map(|(site, f)| (site, f.clone()))
    })
    .flatten()
}

/// The sheet's autofilter as JSON `{r0,c0,r1,c1,cols:[…]}` — the header range
/// plus which column offsets currently carry a rule — or `null` if the sheet
/// has no filter. The host draws a filter button on every header cell in the
/// range and a "filtered" variant on the columns listed.
#[wasm_bindgen]
pub fn session_filter_info(sheet: usize) -> String {
    // Reads in place rather than cloning the filter: the host polls this every
    // frame to decide where to draw the buttons.
    with_session(|s| {
        let sh = s.workbook().sheets.get(sheet)?;
        let f = sh.auto_filter.as_ref()?;
        // Absolute column indices, so the host needs no offset arithmetic.
        let cols: Vec<String> = f
            .rules
            .keys()
            .map(|off| (f.range.start.col.saturating_add(*off)).to_string())
            .collect();
        Some(format!(
            "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{},\"cols\":[{}],\"hidden\":{}}}",
            f.range.start.row,
            f.range.start.col,
            f.range.end.row,
            f.range.end.col,
            cols.join(","),
            sh.filter_hidden.len()
        ))
    })
    .flatten()
    .unwrap_or_else(|| "null".to_owned())
}

/// Every filter region on the sheet, as JSON
/// `{hidden, regions:[{r0,c0,c1,cols:[absCol,…]}, …]}` — the sheet's own filter
/// first, then each table's.
///
/// `hidden` is how many rows the sheet's filters hide between them. It belongs
/// here rather than on `session_filter_info`, which reports nothing at all when
/// the sheet has no filter of its own: a table's filter would then be reported
/// as hiding nothing, and the status line said "filter cleared" on the edit
/// that had just hidden two rows.
///
/// The host draws a button on every header cell in each region and a "filtered"
/// variant on the columns listed. It needs all of them together: a table's
/// buttons are indistinguishable from the sheet's on screen, and drawing a
/// table's from table geometry alone left them unable to say which of its
/// columns carried a rule.
#[wasm_bindgen]
pub fn session_filter_regions(sheet: usize) -> String {
    with_session(|s| {
        let sh = s.workbook().sheets.get(sheet)?;
        let regions: Vec<String> = sheet_filters(sh)
            .map(|(_, f)| {
                let cols: Vec<String> = f
                    .rules
                    .keys()
                    .map(|off| (f.range.start.col.saturating_add(*off)).to_string())
                    .collect();
                format!(
                    "{{\"r0\":{},\"c0\":{},\"c1\":{},\"cols\":[{}]}}",
                    f.range.start.row,
                    f.range.start.col,
                    f.range.end.col,
                    cols.join(",")
                )
            })
            .collect();
        Some(format!(
            "{{\"hidden\":{},\"regions\":[{}]}}",
            sh.filter_hidden.len(),
            regions.join(",")
        ))
    })
    .flatten()
    .unwrap_or_else(|| "{\"hidden\":0,\"regions\":[]}".to_owned())
}

/// Turn an autofilter on over `r0..=r1 × c0..=c1`, treating the first row as
/// the header. Replaces any existing filter, dropping its rules.
#[wasm_bindgen]
pub fn session_set_filter_range(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let range = CellRange::new(
        CellRef::new(r0.min(r1), c0.min(c1)),
        CellRef::new(r1.max(r0), c1.max(c0)),
    );
    commit_filter(sheet, FilterSite::Sheet, Some(AutoFilter::new(range)))
}

/// Turn the autofilter off, releasing every row it hid.
#[wasm_bindgen]
pub fn session_clear_filter(sheet: usize) -> Result<(), JsError> {
    commit_filter(sheet, FilterSite::Sheet, None)
}

/// Drop every column rule but keep the filter (and its buttons) in place.
#[wasm_bindgen]
pub fn session_clear_filter_rules(sheet: usize) -> Result<(), JsError> {
    let Some(mut f) = sheet_filter(sheet) else {
        return Ok(());
    };
    f.rules.clear();
    commit_filter(sheet, FilterSite::Sheet, Some(f))
}

/// The distinct values to offer in column `col`'s checklist, as JSON
/// `{"values":[{"v":…,"c":0|1}],"truncated":0|1,"custom":0|1}`.
///
/// `c` is whether the value is currently checked. The list reflects the rows
/// left by the *other* columns' rules, which is what makes chained filtering
/// behave: filtering Region to "West" leaves only West's cities on offer.
/// `custom` flags that this column carries a condition rather than a checklist,
/// so the host can say so instead of showing every box ticked.
#[wasm_bindgen]
pub fn session_filter_values(sheet: usize, col: u32) -> String {
    let empty = "{\"values\":[],\"truncated\":0,\"custom\":0}".to_owned();
    let out = with_session(|s| {
        let wb = s.workbook();
        let sh = wb.sheets.get(sheet)?;
        let (_, filter) = filter_at_col(sh, col)?;
        let off = col - filter.range.start.col;
        let checked: Option<&Vec<String>> = match filter.rules.get(&off) {
            Some(FilterRule::Values(v)) => Some(v),
            _ => None,
        };
        let custom = matches!(filter.rules.get(&off), Some(FilterRule::Custom { .. }));

        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut truncated = false;
        for row in filter.body_start()..=filter.range.end.row {
            if !row_passes(wb, sh, filter, row, Some(off)) {
                continue;
            }
            if seen.len() >= MAX_FILTER_VALUES {
                truncated = true;
                break;
            }
            seen.insert(filter_operands(wb, sh, row, col).0);
        }
        let items: Vec<String> = seen
            .iter()
            .map(|v| {
                // With no checklist on this column every value is on; with one,
                // only the listed values are.
                let on = checked.is_none_or(|c| c.iter().any(|x| x.eq_ignore_ascii_case(v)));
                format!("{{\"v\":{},\"c\":{}}}", json_string(v), u8::from(on))
            })
            .collect();
        Some(format!(
            "{{\"values\":[{}],\"truncated\":{},\"custom\":{}}}",
            items.join(","),
            u8::from(truncated),
            u8::from(custom)
        ))
    })
    .flatten();
    out.unwrap_or(empty)
}

/// Set column `col` to a checklist of `values`. An
/// empty array clears the column's rule rather than hiding every row — a
/// checklist that selects nothing is a user mistake, not an instruction to
/// blank the sheet.
#[wasm_bindgen]
pub fn session_set_filter_values(
    sheet: usize,
    col: u32,
    values: Vec<String>,
) -> Result<(), JsError> {
    let Some((site, mut f)) = filter_for_col(sheet, col) else {
        return Ok(());
    };
    let off = col - f.range.start.col;
    if values.is_empty() {
        f.rules.remove(&off);
    } else {
        f.rules.insert(off, FilterRule::Values(values));
    }
    commit_filter(sheet, site, Some(f))
}

/// Set column `col` to a condition: `op`/`val` and an optional second
/// `op2`/`val2` joined by AND (`and`) or OR.
///
/// `op` names are the OOXML ones (`equal`, `notEqual`, `greaterThan`,
/// `greaterThanOrEqual`, `lessThan`, `lessThanOrEqual`); "contains",
/// "begins with" and "ends with" are `equal` with the host supplying the
/// wildcards, exactly as Excel stores them. An empty `op` clears the column.
#[wasm_bindgen]
pub fn session_set_filter_custom(
    sheet: usize,
    col: u32,
    op: &str,
    val: &str,
    op2: &str,
    val2: &str,
    and: bool,
) -> Result<(), JsError> {
    let Some((site, mut f)) = filter_for_col(sheet, col) else {
        return Ok(());
    };
    let off = col - f.range.start.col;
    if op.is_empty() {
        f.rules.remove(&off);
    } else {
        f.rules.insert(
            off,
            FilterRule::Custom {
                first: CustomFilter {
                    op: FilterOp::from_ooxml(op),
                    value: val.to_owned(),
                },
                second: (!op2.is_empty()).then(|| CustomFilter {
                    op: FilterOp::from_ooxml(op2),
                    value: val2.to_owned(),
                }),
                and,
            },
        );
    }
    commit_filter(sheet, site, Some(f))
}

/// Re-evaluate every sheet's autofilter against the current data.
///
/// Called after a load, where the rows arrive marked `hidden="1"` with no way
/// to tell which of them the filter hid — OOXML records no distinction. Any row
/// this filter would hide is moved out of the hand-hidden set and into the
/// filter's, so clearing the filter later releases exactly those rows. A row
/// hidden by hand that the filter *also* excludes is reattributed to the
/// filter; Excel cannot tell those apart either.
fn reapply_filters_after_load(session: &mut WorkbookSession) {
    let sheet_count = session.workbook().sheets.len();
    for i in 0..sheet_count {
        let wb = session.workbook();
        let Some(sh) = wb.sheets.get(i) else { continue };
        if sh.auto_filter.as_ref().is_none_or(|f| !f.is_active()) {
            continue;
        }
        let hidden = recompute_filter_hidden(wb, sh);
        // Mutate the loaded document in place: this is reconciling what was
        // read, not an edit, so it must not land on the undo stack or dirty the
        // document.
        if let Some(sh) = session.workbook_mut().sheets.get_mut(i) {
            for row in &hidden {
                sh.hidden_rows.remove(row);
            }
            sh.filter_hidden = hidden;
        }
    }
}

/// Set (or clear, with empty hex) the font color across a range (one undo step).
///
/// `theme_slot` is the `theme="N"` index the colour was picked from, or `-1` for
/// a colour with no theme behind it. Passing the slot is what lets the cell move
/// when the workbook is re-themed; a colour picked off the theme row but stored
/// as bare `RRGGBB` stays put forever.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_font_color(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    hex: &str,
    theme_slot: i32,
    theme_tint: f64,
) -> Result<(), JsError> {
    let color = (!hex.is_empty()).then(|| hex.to_owned());
    let theme = theme_link(theme_slot, theme_tint);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.set_font_color(color.clone(), theme)
    })
}

/// The theme link for a picker's `(slot, tint)`, or `None` when the slot is
/// negative — the editor's way of saying "this colour is not from the theme".
fn theme_link(slot: i32, tint: f64) -> Option<ThemeTint> {
    (slot >= 0).then(|| ThemeTint::from_tint(slot as u32, tint))
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

/// Increase (`delta > 0`) or decrease (`delta < 0`) the number of decimal places
/// across a cell range (atomic undo step).
#[wasm_bindgen]
pub fn session_adjust_decimals(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    delta: i32,
) -> Result<(), JsError> {
    if delta == 0 {
        return Ok(());
    }
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        let current_fmt = st.number_format.as_deref().unwrap_or("General");
        st.number_format = Some(casual_calc_layout::adjust_format_decimals(
            current_fmt,
            delta,
        ));
    })
}

/// The active cell's formatting as JSON (drives the toolbar's active states):
/// `{ b, i, u, al, nf, fc, bg }` — flags present only when set.
#[wasm_bindgen]
pub fn session_cell_format(sheet: usize, row: u32, col: u32) -> String {
    // The workbook default font shown when a cell carries no explicit font — so
    // the toolbar reflects the *effective* font/size (like Excel showing
    // "Calibri"/"11" for an untouched cell) instead of appearing blank.
    const DEFAULT_FONT_NAME: &str = "Calibri";
    const DEFAULT_FONT_PT: f64 = 11.0;
    with_session(|s| {
        let wb = s.workbook();
        let style = wb
            .sheets
            .get(sheet)
            .and_then(|sh| sh.cells.get(CellRef::new(row, col)))
            .and_then(|cell| cell.style)
            .and_then(|id| wb.styles.get(id));
        let mut parts: Vec<String> = Vec::new();
        // Effective font name / size: the cell's own, else the workbook's
        // default font (from the imported styles.xml), else Calibri 11. Always
        // emitted so the toolbar never falls back to a placeholder.
        let font_name = style
            .and_then(|st| st.font_name.clone())
            .or_else(|| wb.default_font_name.clone())
            .unwrap_or_else(|| DEFAULT_FONT_NAME.to_owned());
        let font_pt = style
            .and_then(|st| st.font_size_hp)
            .or(wb.default_font_size_hp)
            .map(|hp| hp as f64 / 2.0)
            .unwrap_or(DEFAULT_FONT_PT);
        parts.push(format!("\"fn\":{}", json_string(&font_name)));
        parts.push(format!("\"fs\":{font_pt}"));
        if let Some(st) = style {
            if st.bold {
                parts.push("\"b\":1".to_owned());
            }
            if st.italic {
                parts.push("\"i\":1".to_owned());
            }
            if st.underline.is_some() {
                parts.push("\"u\":1".to_owned());
            }
            if st.strike {
                parts.push("\"st\":1".to_owned());
            }
            if let Some(v) = st.vert_align {
                parts.push(format!("\"vt\":{}", json_string(v.ooxml())));
            }
            if st.wrap {
                parts.push("\"w\":1".to_owned());
            }
            if st.clip {
                parts.push("\"cl\":1".to_owned());
            }
            if st.indent > 0 {
                parts.push(format!("\"in\":{}", st.indent));
            }
            if st.rotation > 0 {
                parts.push(format!("\"rot\":{}", st.rotation));
            }
            if st.quote_prefix {
                parts.push("\"qp\":1".to_owned());
            }
            if let Some(nf) = st.number_format.as_deref() {
                parts.push(format!("\"nf\":{}", json_string(nf)));
            }
            if let Some(al) = st.align {
                parts.push(format!("\"al\":\"{}\"", al.ooxml()));
            }
            if let Some(va) = st.valign {
                let t = match va {
                    VAlign::Top => "t",
                    VAlign::Middle => "m",
                    VAlign::Bottom => "b",
                    VAlign::Justify => "vj",
                    VAlign::Distributed => "vd",
                };
                parts.push(format!("\"va\":\"{t}\""));
            }
            if let Some(fc) = &st.font_color {
                parts.push(format!("\"fc\":{}", json_string(fc)));
            }
            if let Some(bg) = &st.fill_color {
                parts.push(format!("\"bg\":{}", json_string(bg)));
            }
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

/// Apply a border placement across a range (one undo step) with a chosen line
/// `style` and `color`. `kind` is one of `all`, `inner`, `outer`, `horizontal`,
/// `vertical`, `top`, `bottom`, `left`, `right`, or `none` (clear). `style` is
/// an OOXML line style (`thin`/`medium`/`thick`/`dashed`/`dotted`/`double`);
/// `color` is an `RRGGBB` hex or empty for automatic. Placements other than
/// `none` are additive — they set only the edges they name, leaving the rest.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_border(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
    style: &str,
    color: &str,
) -> Result<(), JsError> {
    let kind = kind.to_owned();
    let style = if style.is_empty() { "thin" } else { style }.to_owned();
    let color = (!color.is_empty()).then(|| color.trim_start_matches('#').to_ascii_uppercase());
    apply_style_range_pos(sheet, r0, c0, r1, c1, move |r, c, st| {
        if kind == "none" {
            st.border = None;
            return;
        }
        let (top, bottom, left, right) = border_edges(&kind, r, c, r0, c0, r1, c1);
        let mut borders = st.border.clone().unwrap_or_default();
        let edge = || {
            Some(BorderEdge {
                style: style.clone(),
                color: color.clone(),
            })
        };
        if top {
            borders.top = edge();
        }
        if bottom {
            borders.bottom = edge();
        }
        if left {
            borders.left = edge();
        }
        if right {
            borders.right = edge();
        }
        // Diagonals are their own placements: one line description plus the
        // direction flags, so "both" draws a cross rather than two borders.
        match kind.as_str() {
            "diagdown" | "diagup" | "diagboth" => {
                borders.diagonal = edge();
                borders.diagonal_down |= kind != "diagup";
                borders.diagonal_up |= kind != "diagdown";
            }
            "nodiag" => {
                borders.diagonal = None;
                borders.diagonal_up = false;
                borders.diagonal_down = false;
            }
            _ => {}
        }
        st.border = (!borders.is_empty()).then_some(borders);
    })
}

/// Which edges `(top, bottom, left, right)` of cell `(r, c)` a placement sets,
/// within the selected range `r0..=r1 × c0..=c1`.
fn border_edges(
    kind: &str,
    r: u32,
    c: u32,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> (bool, bool, bool, bool) {
    match kind {
        "all" => (true, true, true, true),
        "outer" => (r == r0, r == r1, c == c0, c == c1),
        "top" => (r == r0, false, false, false),
        "bottom" => (false, r == r1, false, false),
        "left" => (false, false, c == c0, false),
        "right" => (false, false, false, c == c1),
        // Excel's composite bottoms: the outline plus a heavier or doubled
        // bottom edge, which is how a totals row is conventionally ruled.
        "bottomdouble" | "bottomthick" => (false, r == r1, false, false),
        "topandbottom" => (r == r0, r == r1, false, false),
        // Diagonal placements touch no orthogonal edge.
        "diagdown" | "diagup" | "diagboth" | "nodiag" => (false, false, false, false),
        "inner" => (r > r0, r < r1, c > c0, c < c1),
        "horizontal" => (r > r0, r < r1, false, false),
        "vertical" => (false, false, c > c0, c < c1),
        _ => (false, false, false, false),
    }
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
        ..Borders::default()
    }
}

/// Set (or clear, with empty hex) the solid fill across a range (one undo step).
/// See [`session_set_font_color`] for `theme_slot`.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_fill(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    hex: &str,
    theme_slot: i32,
    theme_tint: f64,
) -> Result<(), JsError> {
    let fill = (!hex.is_empty()).then(|| hex.to_owned());
    let theme = theme_link(theme_slot, theme_tint);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.set_fill_color(fill.clone(), theme)
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

/// Clear the *formats* of a range (fill, borders, number format, font) while
/// keeping each cell's value and formula — the complement of Clear Contents.
/// Cells that carry no style are left untouched, so the op is a no-op when the
/// range has nothing to strip.
#[wasm_bindgen]
pub fn session_clear_formats(
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
                let has_style = session
                    .workbook()
                    .sheets
                    .get(sheet)
                    .and_then(|s| s.cells.get(at))
                    .is_some_and(|cl| cl.style.is_some());
                if has_style {
                    // SetStyle preserves the cell's value and formula.
                    ops.push(EditOperation::SetStyle {
                        sheet,
                        at,
                        style: None,
                    });
                }
            }
        }
        if ops.is_empty() {
            return Ok(());
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
        let vis_cols: Vec<u32> = (c0..=c1).filter(|c| !sh.hidden_cols.contains(c)).collect();
        let mut out = String::new();
        for r in r0..=r1 {
            if sh.is_row_hidden(r) {
                continue; // visible cells only
            }
            for (i, &c) in vis_cols.iter().enumerate() {
                if i > 0 {
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

/// Copy a range as an HTML `<table>` so external apps (Excel, Sheets, mail,
/// docs) receive formatted cells. Paired with the plain-text TSV payload on the
/// OS clipboard; the in-app rich paste still uses the internal clipboard.
#[wasm_bindgen]
pub fn session_copy_html(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return String::new();
        };
        let vis_cols: Vec<u32> = (c0..=c1).filter(|c| !sh.hidden_cols.contains(c)).collect();
        let mut out = String::from("<table>");
        for r in r0..=r1 {
            if sh.is_row_hidden(r) {
                continue; // visible cells only
            }
            out.push_str("<tr>");
            for &c in &vis_cols {
                let cell = sh.cells.get(CellRef::new(r, c));
                let text = cell.map(|cl| display_text(wb, cl)).unwrap_or_default();
                let css = cell
                    .and_then(|cl| cl.style)
                    .and_then(|id| wb.styles.get(id))
                    .map(html_cell_css)
                    .unwrap_or_default();
                if css.is_empty() {
                    out.push_str("<td>");
                } else {
                    out.push_str(&format!("<td style=\"{css}\">"));
                }
                push_html_escaped(&mut out, &text);
                out.push_str("</td>");
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        out
    })
    .unwrap_or_default()
}

/// Inline CSS for one cell's style, for the HTML clipboard payload.
fn html_cell_css(style: &Style) -> String {
    let mut css = String::new();
    if style.bold {
        css.push_str("font-weight:bold;");
    }
    if style.italic {
        css.push_str("font-style:italic;");
    }
    let mut deco = String::new();
    if style.underline.is_some() {
        deco.push_str("underline ");
    }
    if style.strike {
        deco.push_str("line-through");
    }
    let deco = deco.trim();
    if !deco.is_empty() {
        css.push_str(&format!("text-decoration:{deco};"));
    }
    if let Some(c) = &style.font_color {
        css.push_str(&format!("color:#{c};"));
    }
    if let Some(c) = &style.fill_color {
        css.push_str(&format!("background-color:#{c};"));
    }
    if let Some(a) = style.align {
        // CSS has no `fill` or `centerContinuous`, so those fall back to the
        // edge the text starts from — the receiving app gets the placement right
        // even where it cannot reproduce the effect.
        let ta = match a {
            HAlign::Justify | HAlign::Distributed => "justify",
            other => match other.base_edge() {
                HAlign::Center => "center",
                HAlign::Right => "right",
                _ => "left",
            },
        };
        css.push_str(&format!("text-align:{ta};"));
    }
    css
}

fn push_html_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
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

/// A cell captured on the internal clipboard. `dr`/`dc` are the cell's offset
/// among the **visible** cells of the copied range (hidden rows/columns are
/// skipped and the rest compressed), so a paste lands them contiguously.
/// `sr`/`sc` keep the original address for cut-clearing and per-cell formula
/// reference shifting.
struct ClipCell {
    dr: u32,
    dc: u32,
    sr: u32,
    sc: u32,
    cell: Cell,
    formula: Option<Expr>,
}
/// The internal (rich) clipboard: keeps values, styles, and resolved formula
/// ASTs so a paste can reproduce formulas (reference-shifted) and formatting —
/// unlike the text-only OS clipboard.
struct Clip {
    sheet: usize,
    cut: bool,
    cells: Vec<ClipCell>,
}
thread_local! {
    static CLIP: RefCell<Option<Clip>> = const { RefCell::new(None) };
}

/// Snapshot a range onto the internal clipboard (value + style + formula AST).
/// `cut` marks the source to be cleared on the next paste. The OS clipboard TSV
/// is produced separately by [`session_copy_tsv`].
/// Capture the **visible** cells of a range onto clipboard cells: hidden rows
/// and columns are skipped and the survivors compressed to contiguous offsets,
/// so a paste reproduces them with no gaps (the Excel/Sheets default). Pure so
/// it can be unit-tested without a session.
fn clip_capture(
    wb: &Workbook,
    sh: &casual_calc_model::Sheet,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Vec<ClipCell> {
    let vis_rows: Vec<u32> = (r0..=r1).filter(|r| !sh.is_row_hidden(*r)).collect();
    let vis_cols: Vec<u32> = (c0..=c1).filter(|c| !sh.hidden_cols.contains(c)).collect();
    let mut cells = Vec::new();
    for (dr, &r) in vis_rows.iter().enumerate() {
        for (dc, &c) in vis_cols.iter().enumerate() {
            if let Some(cell) = sh.cells.get(CellRef::new(r, c)) {
                let formula = cell.formula.and_then(|h| wb.formula(h)).cloned();
                cells.push(ClipCell {
                    dr: dr as u32,
                    dc: dc as u32,
                    sr: r,
                    sc: c,
                    cell: cell.clone(),
                    formula,
                });
            }
        }
    }
    cells
}

#[wasm_bindgen]
pub fn session_clip_copy(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32, cut: bool) {
    let _ = with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return;
        };
        let cells = clip_capture(wb, sh, r0, c0, r1, c1);
        CLIP.with(|cl| *cl.borrow_mut() = Some(Clip { sheet, cut, cells }));
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
/// `"formats"` (the source style only, keeping the target's value),
/// `"formulas"` (value + formula, reference-shifted, keeping the target's
/// formatting), or `"transpose"` (a full paste with rows and columns swapped).
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
        let transpose = mode == "transpose";
        let (ops, was_cut, empty) = CLIP.with(|cl| {
            let borrow = cl.borrow();
            let Some(clip) = borrow.as_ref() else {
                return (Vec::new(), false, true);
            };
            let cut = clip.cut && mode == "all";
            let mut ops = Vec::new();
            if cut {
                for cc in &clip.cells {
                    ops.push(EditOperation::ClearCell {
                        sheet: clip.sheet,
                        at: CellRef::new(cc.sr, cc.sc),
                    });
                }
            }
            for cc in &clip.cells {
                // Transpose swaps the row/column offsets so the block lands
                // rotated about its top-left origin.
                let at = if transpose {
                    CellRef::new(row + cc.dc, col + cc.dr)
                } else {
                    CellRef::new(row + cc.dr, col + cc.dc)
                };
                match mode {
                    // Arithmetic paste: combine the copied number with what is
                    // already there. Anything non-numeric on either side is left
                    // alone rather than coerced to zero, which would silently
                    // turn a label into a number.
                    "add" | "subtract" | "multiply" | "divide" => {
                        let CellValue::Number(src) = cc.cell.value else {
                            return (ops, cut, false);
                        };
                        let target = session
                            .workbook()
                            .sheets
                            .get(sheet)
                            .and_then(|s| s.cells.get(at))
                            .map(|c| c.value.clone())
                            .unwrap_or(CellValue::Empty);
                        // An empty target is the identity for the operation, so
                        // pasting onto blanks behaves like a plain paste.
                        let base = match target {
                            CellValue::Number(n) => n,
                            CellValue::Empty => match mode {
                                "multiply" | "divide" => 1.0,
                                _ => 0.0,
                            },
                            _ => return (ops, cut, false),
                        };
                        let value = match mode {
                            "add" => base + src,
                            "subtract" => base - src,
                            "multiply" => base * src,
                            // Division by zero yields Excel's own error rather
                            // than an infinity the grid cannot render.
                            _ if src == 0.0 => {
                                ops.push(EditOperation::SetValue {
                                    sheet,
                                    at,
                                    value: CellValue::Error(casual_calc_model::ErrorValue::Div0),
                                });
                                continue;
                            }
                            _ => base / src,
                        };
                        ops.push(EditOperation::SetValue {
                            sheet,
                            at,
                            value: CellValue::Number(value),
                        });
                    }
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
                    "formulas" => {
                        // Value + formula (reference-shifted), but keep the
                        // target cell's existing style. Read the target style
                        // first (StyleId is Copy, so the borrow ends here).
                        let target_style = session
                            .workbook()
                            .sheets
                            .get(sheet)
                            .and_then(|s| s.cells.get(at))
                            .and_then(|c| c.style);
                        let mut out = cc.cell.clone();
                        out.style = target_style;
                        if let Some(expr) = &cc.formula {
                            let dr = at.row as i64 - cc.sr as i64;
                            let dc = at.col as i64 - cc.sc as i64;
                            let shifted = shift_references(expr, dr, dc);
                            out.formula = Some(session.workbook_mut().store_formula(shifted));
                        }
                        ops.push(EditOperation::SetCell {
                            sheet,
                            at,
                            cell: Some(out),
                        });
                    }
                    _ => {
                        let mut out = cc.cell.clone();
                        if let Some(expr) = &cc.formula {
                            // Each cell moved from (sr,sc) to `at`; shift its
                            // references by that per-cell delta (uniform when
                            // nothing was compressed away).
                            let dr = at.row as i64 - cc.sr as i64;
                            let dc = at.col as i64 - cc.sc as i64;
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

/// Copy one cell's whole style onto a range, leaving values and formulas alone
/// — the format painter. Copying the *resolved* style rather than replaying
/// individual toolbar ops is what makes it faithful: number format, font, fill,
/// borders, alignment and wrap all travel together.
#[wasm_bindgen]
pub fn session_copy_style(
    sheet: usize,
    src_row: u32,
    src_col: u32,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let source = with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.cells.get(CellRef::new(src_row, src_col)))
            .and_then(|cell| cell.style)
            .and_then(|id| s.workbook().styles.get(id))
            .cloned()
    })
    .flatten();
    // An unstyled source clears the target's formatting, which is what painting
    // from a plain cell should do.
    let source = source.unwrap_or_default();
    apply_style_range(sheet, r0, c0, r1, c1, move |st| *st = source.clone())
}

/// Delete duplicate rows within a range, keeping the first occurrence of each,
/// and return how many were removed.
///
/// Rows are compared on their *displayed* values across the range's columns —
/// what the user sees is what "duplicate" means; two cells reading `1.50` are
/// the same row even if one is a formula. Later rows shift up, as with a row
/// delete, so a table stays contiguous. `first_row` lets the caller exclude a
/// header.
#[wasm_bindgen]
pub fn session_remove_duplicates(
    sheet: usize,
    first_row: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<u32, JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut dupes: Vec<u32> = Vec::new();
        if let Some(sh) = session.workbook().sheets.get(sheet) {
            let wb = session.workbook();
            for r in first_row..=r1 {
                let mut key = String::new();
                for c in c0..=c1 {
                    if let Some(cell) = sh.cells.get(CellRef::new(r, c)) {
                        key.push_str(&display_text(wb, cell));
                    }
                    key.push('\u{1}'); // separator no cell text can contain
                }
                if !seen.insert(key) {
                    dupes.push(r);
                }
            }
        } else {
            return Ok(0);
        }
        if dupes.is_empty() {
            return Ok(0);
        }
        // Delete bottom-up so each index still refers to the intended row.
        let mut ops = Vec::with_capacity(dupes.len());
        for r in dupes.iter().rev() {
            ops.push(EditOperation::DeleteRows {
                sheet,
                at: *r,
                count: 1,
            });
        }
        let removed = dupes.len() as u32;
        session.edit(EditOperation::Batch(ops)).map_err(js)?;
        Ok(removed)
    })
}

/// Set text rotation over a range, in OOXML's `textRotation` encoding (0–90
/// counter-clockwise, 91–180 for `value - 90` clockwise, 255 stacked).
#[wasm_bindgen]
pub fn session_set_rotation(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    rotation: u16,
) -> Result<(), JsError> {
    let rot = rotation.min(255);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| st.rotation = rot)
}

/// Step the indent of a range by `delta` levels, clamped to Excel's 0–250.
#[wasm_bindgen]
pub fn session_adjust_indent(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    delta: i32,
) -> Result<(), JsError> {
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.indent = (i32::from(st.indent) + delta).clamp(0, 250) as u8;
    })
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

/// What undo would reverse, for a menu label, or empty when there is nothing.
#[wasm_bindgen]
pub fn session_undo_label() -> String {
    with_session(|s| s.undo_label().unwrap_or_default().to_owned()).unwrap_or_default()
}

/// What redo would reapply, or empty.
#[wasm_bindgen]
pub fn session_redo_label() -> String {
    with_session(|s| s.redo_label().unwrap_or_default().to_owned()).unwrap_or_default()
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

/// Whether a format code is the Text format — a single `@` section, i.e. "keep
/// whatever was typed as text". A four-section code's text section does not
/// make the *cell* text; it only styles text that is already there.
fn is_text_format(code: &str) -> bool {
    let trimmed = code.trim();
    !trimmed.is_empty() && !trimmed.contains(';') && trimmed.contains('@')
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

    // A leading apostrophe forces the rest to be text, however numeric it
    // looks, and is not part of the value. The marker has to be recorded on the
    // style (`quotePrefix`), not merely obeyed here: without it the cell saves
    // as a plain string and Excel re-reads `0123` as the number 123 the next
    // time the file is opened.
    if let Some(body) = trimmed.strip_prefix('\'') {
        let mut style = existing_style
            .and_then(|id| session.workbook().styles.get(id))
            .cloned()
            .unwrap_or_default();
        style.quote_prefix = true;
        let style = session.workbook_mut().intern_style(style);
        let text = session.workbook_mut().intern_string(body);
        let mut cell = Cell::value(CellValue::InlineString(text));
        cell.style = Some(style);
        return EditOperation::SetCell {
            sheet,
            at,
            cell: Some(cell),
        };
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

    // A cell formatted as Text (`@`) keeps what was typed as text — that is the
    // entire point of the format, and coercing "007" or "1-2" to a number here
    // is a silent edit of what the user entered.
    let text_formatted = existing_style
        .and_then(|id| session.workbook().styles.get(id))
        .and_then(|st| st.number_format.as_deref())
        .is_some_and(is_text_format);
    // An ISO date becomes a real date, keeping the same rules the importer uses
    // so that typing a date and pasting one from a file agree. It brings its own
    // format, since a bare serial displayed as a number is not what was typed.
    if !text_formatted && let Some((serial, code)) = casual_calc_io::parse_iso_datetime(trimmed) {
        // An existing date format wins — someone who set dd/mm/yyyy on the
        // column means it, and retyping a cell should not reset the column.
        let keep = existing_style
            .and_then(|id| session.workbook().styles.get(id))
            .and_then(|st| st.number_format.as_deref())
            .is_some_and(casual_calc_io::is_date_format);
        let style = if keep {
            existing_style
        } else {
            let mut style = existing_style
                .and_then(|id| session.workbook().styles.get(id))
                .cloned()
                .unwrap_or_default();
            style.number_format = Some(code.to_owned());
            Some(session.workbook_mut().intern_style(style))
        };
        let mut cell = Cell::value(CellValue::Number(serial));
        cell.style = style;
        return EditOperation::SetCell {
            sheet,
            at,
            cell: Some(cell),
        };
    }

    let value = match trimmed.parse::<f64>() {
        Ok(n) if !text_formatted && !has_leading_zero(trimmed) => CellValue::Number(n),
        _ => CellValue::InlineString(session.workbook_mut().intern_string(trimmed)),
    };
    EditOperation::SetValue { sheet, at, value }
}

/// Whether typed input carries a padding zero (`007`), which means it is an
/// identifier rather than a quantity and must not be flattened to a number.
fn has_leading_zero(input: &str) -> bool {
    let digits = input.strip_prefix(['+', '-']).unwrap_or(input);
    let mut chars = digits.chars();
    chars.next() == Some('0') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

/// A cell's editable content: `=formula` for a formula cell, otherwise the
/// value as it would be typed. Find & Replace operate on this (Excel's default
/// "Formulas" look-in) so a match is always something Replace can rewrite.
fn cell_input_text(wb: &Workbook, cell: &Cell) -> String {
    if let Some(handle) = cell.formula
        && let Some(expr) = wb.formula(handle)
    {
        return format!("={expr}");
    }
    value_text(wb, &cell.value)
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
    // One diagonal line description, plus which way (or ways) it runs.
    edge("d", &b.diagonal);
    if b.diagonal_up {
        parts.push("\"du\":1".to_owned());
    }
    if b.diagonal_down {
        parts.push("\"dd\":1".to_owned());
    }
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

#[cfg(test)]
mod tests {
    use super::{html_cell_css, push_html_escaped};
    use casual_calc_model::{HAlign, Style};

    #[test]
    fn html_escape_covers_markup_chars() {
        let mut out = String::new();
        push_html_escaped(&mut out, r#"a<b>&"c"#);
        assert_eq!(out, "a&lt;b&gt;&amp;&quot;c");
    }

    #[test]
    fn cell_css_maps_style_to_inline_css() {
        let style = Style {
            bold: true,
            italic: true,
            strike: true,
            font_color: Some("FF0000".to_owned()),
            fill_color: Some("FFFF00".to_owned()),
            align: Some(HAlign::Center),
            ..Style::default()
        };
        let css = html_cell_css(&style);
        assert!(css.contains("font-weight:bold;"));
        assert!(css.contains("font-style:italic;"));
        assert!(css.contains("text-decoration:line-through;"));
        assert!(css.contains("color:#FF0000;"));
        assert!(css.contains("background-color:#FFFF00;"));
        assert!(css.contains("text-align:center;"));
    }

    #[test]
    fn cell_css_is_empty_for_default_style() {
        assert_eq!(html_cell_css(&Style::default()), "");
    }

    #[test]
    fn clip_capture_skips_hidden_rows_and_compresses() {
        use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
        let wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        for r in 0..4u32 {
            sheet.cells.set(
                CellRef::new(r, 0),
                Cell::value(CellValue::Number((r + 1) as f64)),
            );
        }
        sheet.hidden_rows.insert(1); // hide the second row

        let clip = super::clip_capture(&wb, &sheet, 0, 0, 3, 0);
        // Row 1 is skipped; the three survivors compress to dr 0,1,2 while
        // keeping their true source rows for cut/formula math.
        assert_eq!(clip.len(), 3);
        assert_eq!((clip[0].sr, clip[0].dr), (0, 0));
        assert_eq!((clip[1].sr, clip[1].dr), (2, 1));
        assert_eq!((clip[2].sr, clip[2].dr), (3, 2));
        assert_eq!(clip[1].cell.value, CellValue::Number(3.0));
    }

    /// Undo must reverse the sheet-metadata edit itself, not whatever preceded
    /// it.
    ///
    /// These six areas used to write straight to `workbook_mut()`. That is
    /// worse than having no undo: the button stays enabled and the history
    /// keeps filling, so Ctrl+Z after adding a comment silently reversed the
    /// *previous cell edit* — destroying work the user never touched, in a
    /// place they were not looking. This asserts the cell survives and the
    /// metadata change is the thing that goes.
    #[test]
    fn undo_reverses_metadata_edits_not_the_edit_before_them() {
        use super::{
            session_add_cf, session_cell_input, session_comment_at, session_new, session_set_cell,
            session_set_comment, session_set_sheet_protected, session_set_sheet_visibility,
            session_undo,
        };
        for (label, apply) in [
            (
                "comment",
                (&|| {
                    session_set_comment(0, 5, 5, "note", "", "").unwrap();
                }) as &dyn Fn(),
            ),
            ("conditional format", &|| {
                session_add_cf(0, 0, 0, 3, 3, "gt", 5.0, 0.0, "", "FF0000").unwrap();
            }),
            ("sheet protection", &|| {
                session_set_sheet_protected(0, true).unwrap();
            }),
        ] {
            session_new();
            session_set_cell(0, 0, 0, "keep me").unwrap();
            apply();
            session_undo().unwrap();
            assert_eq!(
                session_cell_input(0, 0, 0),
                "keep me",
                "undo after a {label} edit destroyed the preceding cell edit"
            );
        }

        // And the metadata change itself is what undo removes.
        session_new();
        session_set_comment(0, 1, 1, "hello", "", "").unwrap();
        assert_eq!(session_comment_at(0, 1, 1), "hello");
        session_undo().unwrap();
        assert_eq!(session_comment_at(0, 1, 1), "");

        // Hiding a sheet is reversible too; it used to be permanent.
        session_new();
        super::session_add_sheet().unwrap();
        session_set_sheet_visibility(1, "hidden").unwrap();
        // The reader returns a JSON array of every sheet's state.
        assert!(super::session_sheet_visibility().contains("hidden"));
        session_undo().unwrap();
        assert!(!super::session_sheet_visibility().contains("hidden"));
    }

    /// Typing a date must produce a date, and a date cell must edit as one —
    /// the serial is an implementation detail that should never surface.
    #[test]
    fn typed_dates_and_identifiers_keep_their_meaning() {
        use super::{
            session_cell_format, session_cell_input, session_new, session_set_cell,
            session_set_number_format,
        };
        session_new();
        session_set_cell(0, 0, 0, "2024-03-05").unwrap();
        session_set_cell(0, 1, 0, "13:45").unwrap();
        session_set_cell(0, 2, 0, "007").unwrap();
        session_set_cell(0, 3, 0, "1234.5").unwrap();

        // Round-trips through the formula bar as what was typed.
        assert_eq!(session_cell_input(0, 0, 0), "2024-03-05");
        assert_eq!(session_cell_input(0, 1, 0), "13:45");
        // A padding zero marks an identifier, so it survives.
        assert_eq!(session_cell_input(0, 2, 0), "007");
        // A plain number is untouched and keeps showing as a number.
        assert_eq!(session_cell_input(0, 3, 0), "1234.5");
        // And the date really is a serial underneath, so arithmetic works.
        assert!(session_cell_format(0, 0, 0).contains("\"nf\":\"yyyy-mm-dd\""));

        // A leading apostrophe forces text and records the marker, so the
        // value survives a save instead of reverting to a number on reopen.
        session_set_cell(0, 5, 0, "'0123").unwrap();
        assert_eq!(session_cell_input(0, 5, 0), "'0123");
        assert!(session_cell_format(0, 5, 0).contains("\"qp\":1"));

        // Retyping a date under a format the user chose keeps their format
        // rather than resetting the cell to the ISO one.
        session_set_number_format(0, 4, 0, 4, 0, "dd/mm/yyyy").unwrap();
        session_set_cell(0, 4, 0, "2024-03-05").unwrap();
        assert!(session_cell_format(0, 4, 0).contains("\"nf\":\"dd/mm/yyyy\""));
        assert_eq!(session_cell_input(0, 4, 0), "05/03/2024");
    }

    // Drives the real session_* functions (thread-local SESSION/CLIP) natively
    // to exercise the M3-3 paste-special modes end to end.
    #[test]
    fn paste_special_transpose_and_formulas() {
        use super::{
            session_cell_format, session_cell_input, session_clip_copy, session_clip_paste_mode,
            session_new, session_set_cell, session_toggle_bold,
        };
        // --- Transpose: a 2x2 block pasted rotated about its top-left. ---
        session_new();
        session_set_cell(0, 0, 0, "1").unwrap(); // A1
        session_set_cell(0, 0, 1, "2").unwrap(); // B1
        session_set_cell(0, 1, 0, "=A1*10").unwrap(); // A2 (a formula)

        session_clip_copy(0, 0, 0, 1, 1, false); // copy A1:B2
        session_clip_paste_mode(0, 4, 0, "transpose").unwrap(); // top-left at A5
        assert_eq!(session_cell_input(0, 4, 0), "1"); // A5  (A1 stays at origin)
        assert_eq!(session_cell_input(0, 5, 0), "2"); // A6  (B1 → below origin)
        // A2's formula transposes to B5; it moved (dr=+3, dc=+1), so =A1*10 → B4*10.
        assert_eq!(session_cell_input(0, 4, 1), "=(B4*10)"); // B5

        // --- Formulas-only: value+formula in, target's formatting kept. ---
        session_new();
        session_set_cell(0, 0, 0, "5").unwrap(); // A1
        session_set_cell(0, 1, 0, "=A1+1").unwrap(); // A2 formula
        session_set_cell(0, 4, 3, "9").unwrap(); // D5 target
        session_toggle_bold(0, 4, 3, 4, 3).unwrap(); // bold D5
        session_clip_copy(0, 1, 0, 1, 0, false); // copy A2
        session_clip_paste_mode(0, 4, 3, "formulas").unwrap(); // onto D5
        // A2 moved to D5 (dr=+3, dc=+3): =A1+1 → =(D4+1).
        assert_eq!(session_cell_input(0, 4, 3), "=(D4+1)");
        // The target's bold formatting is preserved (formulas-only ignores source style).
        assert!(
            session_cell_format(0, 4, 3).contains("\"b\":1"),
            "formulas-only paste dropped the target's bold"
        );
    }
}

#[cfg(test)]
mod fill_series_tests {
    use super::{
        detect_text_series, session_cell_input, session_fill, session_new, session_set_cell,
        text_series_at,
    };

    #[test]
    fn text_series_detection() {
        // A single month name is a series (step +1); mixed lists are not.
        assert_eq!(detect_text_series(&[Some("Jan".into())]), Some((1, 0, 1)));
        assert_eq!(
            detect_text_series(&[Some("Jan".into()), Some("Feb".into())]),
            Some((1, 0, 1))
        );
        // Descending wraps: Dec, Nov → step 11 (== -1 mod 12).
        assert_eq!(
            detect_text_series(&[Some("Dec".into()), Some("Nov".into())]),
            Some((1, 11, 11))
        );
        assert_eq!(
            detect_text_series(&[Some("Jan".into()), Some("Mon".into())]),
            None
        );
        assert_eq!(detect_text_series(&[Some("hello".into())]), None);
        // Extension wraps December → January.
        assert_eq!(text_series_at(1, 10, 1, 2), "Jan"); // Nov(10) + 2 steps = 12 mod 12 = 0 = Jan
        assert_eq!(text_series_at(1, 11, 1, 1), "Jan"); // Dec(11) + 1 → Jan (wrap)
    }

    #[test]
    fn fill_extends_month_names() {
        session_new();
        session_set_cell(0, 0, 0, "Jan").unwrap(); // A1
        session_set_cell(0, 1, 0, "Feb").unwrap(); // A2
        // Drag A1:A2 down to A5 → Mar, Apr, May.
        session_fill(0, 0, 0, 1, 0, 0, 0, 4, 0).unwrap();
        assert_eq!(session_cell_input(0, 2, 0), "Mar");
        assert_eq!(session_cell_input(0, 3, 0), "Apr");
        assert_eq!(session_cell_input(0, 4, 0), "May");

        // A single weekday name also extends (and wraps).
        session_new();
        session_set_cell(0, 0, 0, "Fri").unwrap(); // A1
        session_fill(0, 0, 0, 0, 0, 0, 0, 2, 0).unwrap(); // A1 down to A3
        assert_eq!(session_cell_input(0, 1, 0), "Sat");
        assert_eq!(session_cell_input(0, 2, 0), "Sun"); // wraps Sat → Sun
    }
}
