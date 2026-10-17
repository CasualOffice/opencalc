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

/// Search a sheet — or the whole workbook — with the options a find bar offers,
/// as JSON `[{r,c,s}, …]`.
///
/// - `whole_cell`: the cell must equal the query, not merely contain it.
/// - `in_values`: match what the cell *displays* (Excel's "Values" look-in)
///   rather than what you would type into it.
/// - `all_sheets`: search every sheet, tagging each hit with its sheet index.
///
/// A wrapper over [`session_find_ex`]'s search, kept for callers of the older
/// signature; there is one matcher.
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
    let opts = crate::find::FindOptions {
        match_case,
        whole_cell,
        look_in: if in_values {
            crate::find::LookIn::Values
        } else {
            crate::find::LookIn::Formulas
        },
        all_sheets,
        by_columns: false,
        wildcards,
        range: None,
    };
    with_session(|s| {
        let pattern = crate::find::Pattern::new(query, wildcards, match_case);
        let hits: Vec<String> = crate::find::scan(s.workbook(), sheet, &pattern, &opts)
            .into_iter()
            .map(|h| {
                format!(
                    "{{\"r\":{},\"c\":{},\"s\":{}}}",
                    h.at.row, h.at.col, h.sheet
                )
            })
            .collect();
        format!("[{}]", hits.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Replace `find` with `replace` in every cell of one sheet, returning the
/// count (one undo step). Literal text, substring match — the older signature,
/// now a wrapper over [`session_replace_all_ex`].
#[wasm_bindgen]
pub fn session_replace_all(
    sheet: usize,
    find: &str,
    replace: &str,
    match_case: bool,
) -> Result<usize, JsError> {
    let opts = format!("{{\"matchCase\":{match_case},\"wildcards\":false}}");
    session_replace_all_ex(sheet, find, replace, &opts)
}

/// Replace occurrences of `find` in a single cell's editable text, re-parsing
/// the result. Returns whether the cell changed. A wrapper over
/// [`session_replace_at_ex`].
#[wasm_bindgen]
pub fn session_replace_at(
    sheet: usize,
    row: u32,
    col: u32,
    find: &str,
    replace: &str,
    match_case: bool,
) -> Result<bool, JsError> {
    let opts = format!("{{\"matchCase\":{match_case},\"wildcards\":false}}");
    session_replace_at_ex(sheet, row, col, find, replace, &opts)
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
    guard_protected(sheet, rr0, cc0, rr1, cc1)?;
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
        let op = session.input_edit(sheet, CellRef::new(row, col), input);
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
///
/// Which rows and columns go is `copied_axes`'s answer: everything in the
/// range except rows a filter has filtered out — a row or column hidden by
/// hand is copied, as in Excel (XL-UX-176).
#[wasm_bindgen]
pub fn session_copy_tsv(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    with_session(|s| {
        let (rows, cols) = copied_axes(s, sheet, r0, c0, r1, c1);
        tsv_of(s.workbook(), sheet, &rows, &cols)
    })
    .unwrap_or_default()
}

/// [`session_copy_tsv`] over exactly the rows and columns given — the copy of
/// a multi-area selection whose areas form one grid, such as Go To Special ▸
/// Visible cells only leaves. Nothing is skipped: the caller chose the cells.
#[wasm_bindgen]
#[must_use]
pub fn session_copy_tsv_grid(sheet: usize, rows: Vec<u32>, cols: Vec<u32>) -> String {
    with_session(|s| tsv_of(s.workbook(), sheet, &sorted(rows), &sorted(cols))).unwrap_or_default()
}

pub(crate) fn tsv_of(wb: &Workbook, sheet: usize, rows: &[u32], cols: &[u32]) -> String {
    let Some(sh) = wb.sheets.get(sheet) else {
        return String::new();
    };
    let mut out = String::new();
    for &r in rows {
        for (i, &c) in cols.iter().enumerate() {
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
}

/// The rows and columns a copy of `r0..=r1 × c0..=c1` takes, in order.
///
/// Excel copies a row or column hidden **by hand** with the rest — "By
/// default, Excel copies hidden or filtered cells in addition to visible
/// cells" (Microsoft, "Copy visible cells only") — and leaves out the rows an
/// AutoFilter has filtered out, which is what Excel for Microsoft 365 does on
/// a plain Ctrl+C of a filtered list (the "or filtered" in that sentence is
/// disputed on Microsoft's own Tech Community and does not match the
/// desktop application). A participant's personal view is a filter too, so
/// its rows are left out the same way. To copy only what is on screen in any
/// other case, Go To Special ▸ Visible cells only first.
///
/// This used to skip every hidden row and column (`M3-5`), so A1:A4 with row
/// 2 hidden by hand pasted three rows.
pub(crate) fn copied_axes(
    s: &WorkbookSession,
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> (Vec<u32>, Vec<u32>) {
    let (r0, r1, c0, c1) = (r0.min(r1), r0.max(r1), c0.min(c1), c0.max(c1));
    let Some(sh) = s.workbook().sheets.get(sheet) else {
        return (Vec::new(), Vec::new());
    };
    let rows = (r0..=r1)
        .filter(|r| !sh.filter_hidden.contains(r) && !s.views().hides(sheet, *r))
        .collect();
    (rows, (c0..=c1).collect())
}

/// Sorted and without repeats, so a grid handed in from the host is read in
/// sheet order whatever order the host built it in.
pub(crate) fn sorted(mut v: Vec<u32>) -> Vec<u32> {
    v.sort_unstable();
    v.dedup();
    v
}

/// Who last changed a cell, or empty (`HIST-02`).
///
/// Returns the display name. The host's opaque id is what the model *compares*
/// — two people called Alex are two authors — but it is not something to show
/// somebody, and a binding that returned it would invite a host to draw it.
///
/// Empty in every local session, where there is one author and it is you, and
/// empty for every cell of a file just opened: attribution does not survive a
/// round-trip through `.xlsx`, because OOXML revision tracking is a different
/// and larger format. The editor says "since this file was opened" where it
/// shows the name, rather than letting an absence read as "nobody".
#[wasm_bindgen]
#[must_use]
pub fn session_cell_author(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return String::new();
        };
        sh.attribution
            .get(&CellRef::new(row, col))
            .and_then(|id| wb.authors.get(*id))
            .map(|a| a.name.clone())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// Tell the engine who this session's edits are by (`HIST-02`).
///
/// `id` is the host's own opaque key — its user id, a JWT subject, anything —
/// and is what identity is decided by. `name` is for display only. A host that
/// has no id passes an empty one, and the name becomes the identity, which is
/// exactly what a host supplying nothing has today.
///
/// Passing an empty name clears the author, which is what a session that stops
/// being collaborative should do rather than keep stamping a stale name.
#[wasm_bindgen]
pub fn session_set_author(id: &str, name: &str) {
    let _ = with_session_mut(|s| {
        if name.trim().is_empty() {
            s.set_author(None);
        } else {
            s.set_author(Some(casual_calc_model::Author {
                id: id.to_owned(),
                name: name.to_owned(),
            }));
        }
        Ok(())
    });
}

#[cfg(test)]
mod copy_hidden_tests {
    use super::{session_copy_tsv, session_copy_tsv_grid};
    use crate::{
        session_clip_copy, session_clip_paste, session_hide_cols, session_hide_rows, session_new,
        session_set_cell, session_set_filter_range, session_set_filter_values,
    };

    /// XL-UX-176: a row or column hidden by hand is copied, as in Excel.
    #[test]
    fn rows_and_columns_hidden_by_hand_are_copied() {
        session_new();
        for (r, v) in ["one", "two", "three"].iter().enumerate() {
            session_set_cell(0, r as u32, 0, v).unwrap();
            session_set_cell(0, r as u32, 1, &(r + 1).to_string()).unwrap();
        }
        session_hide_rows(0, 1, 1).unwrap();
        session_hide_cols(0, 0, 0).unwrap();
        assert_eq!(
            session_copy_tsv(0, 0, 0, 2, 1),
            "one\t1\ntwo\t2\nthree\t3\n"
        );
        session_clip_copy(0, 0, 0, 2, 0, false);
        session_clip_paste(0, 0, 3).unwrap();
        assert_eq!(
            crate::session_cell_input(0, 1, 3),
            "two",
            "the paste has the hidden row"
        );
    }

    /// …and a row a filter filtered out is not.
    #[test]
    fn rows_a_filter_hides_are_left_out() {
        session_new();
        for (r, v) in ["head", "one", "two", "three"].iter().enumerate() {
            session_set_cell(0, r as u32, 0, v).unwrap();
        }
        session_set_filter_range(0, 0, 0, 3, 0).unwrap();
        session_set_filter_values(0, 0, vec!["one".into(), "three".into()]).unwrap();
        assert_eq!(session_copy_tsv(0, 0, 0, 3, 0), "head\none\nthree\n");
        // A grid the host chose is copied as given, in sheet order.
        assert_eq!(
            session_copy_tsv_grid(0, vec![3, 0, 3], vec![0]),
            "head\nthree\n"
        );
    }
}
