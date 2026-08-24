//! Reading and clearing cells: values, inputs, extents, find and replace.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// One viewport's worth of paint instructions, as JSON.
///
/// **Measurement first, bridge second.** `RND-10` defers sharing the display
/// list between the canvas and the PNG renderer on the grounds that "a naive
/// per-frame serialisation would be slower than what it replaces". The native
/// half of that is now measured — `display-list-frame-serialise`, a median
/// 178 µs against a 16.67 ms frame — and the half that was still a guess is the
/// WASM→JS crossing itself, which nothing could measure because nothing crossed.
///
/// This is the smallest thing that makes that measurable: the same
/// `layout_viewport` the PNG renderer uses, serialised once. It is deliberately
/// **not** wired into the canvas — the editor still paints from its per-cell
/// payload, so this changes no drawing and cannot regress a frame. Whether the
/// canvas moves onto it is the decision this exists to inform, not one it makes.
///
/// Pixels and a dpi rather than the layout's own units, because every other
/// geometry binding here takes pixels and a caller that had to convert would be
/// the one place that did.
#[wasm_bindgen]
pub fn session_display_list(
    sheet: usize,
    width_px: u32,
    height_px: u32,
    dpi: u32,
) -> Result<String, JsError> {
    with_session(|s| {
        let workbook = s.workbook();
        let Some(sheet_ref) = workbook.sheets.get(sheet) else {
            return Ok(String::from(r#"{"items":[]}"#));
        };
        let geometry = casual_calc_layout::GridGeometry::for_sheet(sheet_ref);
        let viewport = crate::viewport_px(width_px, height_px, dpi);
        let list = casual_calc_layout::layout_viewport(workbook, sheet, &geometry, &viewport);
        serde_json::to_string(&list).map_err(|why| JsError::new(&format!("display list: {why}")))
    })
    .unwrap_or_else(|| Ok(String::from(r#"{"items":[]}"#)))
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

pub(crate) fn commit_edit(op: EditOperation) -> Result<(), JsError> {
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
pub(crate) fn current_sheet_metadata(
    session: &WorkbookSession,
    sheet: usize,
) -> Option<EditOperation> {
    let sh = session.workbook().sheets.get(sheet)?;
    Some(EditOperation::set_sheet_metadata(
        sheet,
        SheetMetadata::capture(sh),
    ))
}

/// Insert `count` blank rows before `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_insert_rows(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    guard_protected(sheet, at, 0, at.saturating_add(count.max(1) - 1), 0)?;
    commit_edit(EditOperation::InsertRows { sheet, at, count })
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

/// Set a cell from user input (a formula `=…`, a number, or text), then recalc.
#[wasm_bindgen]
pub fn session_set_cell(sheet: usize, row: u32, col: u32, input: &str) -> Result<(), JsError> {
    guard_protected(sheet, row, col, row, col)?;
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
    guard_protected(sheet, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))?;
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
    guard_protected(sheet, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))?;
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
