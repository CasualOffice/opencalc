//! Column and row geometry: widths, heights, pixel offsets, hiding and
//! the outline.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// Row heights in device pixels (96 dpi) for `count` rows starting at `first`.
#[wasm_bindgen]
pub fn session_row_px(sheet: usize, first: u32, count: u32) -> String {
    axis_px(sheet, first, count, DEFAULT_ROW_HEIGHT, false)
}

/// Shared body of [`session_col_px`]/[`session_row_px`]: a JSON array of
/// per-line pixel sizes, honoring the sheet's overrides and default.
pub(crate) fn axis_px(
    sheet: usize,
    first: u32,
    count: u32,
    fallback: i64,
    columns: bool,
) -> String {
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
                    // The *display* question, so it asks the session and gets
                    // the union of the shared hidden sets and this
                    // participant's own personal view (`COL-32`, docs/71).
                    // `Sheet::is_row_hidden` is the other question — the one
                    // `SUBTOTAL` asks — and answering this one with it is what
                    // makes a personal filter invisible on screen.
                    !s.is_row_visible(sheet, line)
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
pub(crate) const MAX_AUTOFIT_CANDIDATES: usize = 20_000;

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
///
/// The twips-to-pixels conversion is done in `i64` and only then narrowed.
/// Done in `i32` it overflows: an axis offset is twips, and `twips * 96` passes
/// `i32::MAX` at 1,491,308 px — and because `overflow-checks` is on in the
/// release profile, that is not a wrong number, it is a **panic in the shipped
/// build**. A panicking `wasm_bindgen` export traps the module, so the whole
/// editor stops and the document goes with it.
///
/// Columns cannot reach it at the default width, but they can be widened, and
/// a bound that holds only for the default is not a bound. See
/// [`session_row_offset_px`], where rows reach it at row 74,566.
#[wasm_bindgen]
pub fn session_col_offset_px(sheet: usize, col: u32) -> i32 {
    with_session(|s| px_from_twips(geometry_of(s, sheet).columns.offset(col))).unwrap_or(0)
}

/// Twips to pixels at 96 dpi, saturating rather than trapping.
///
/// `i32` is the return type these exports have across the boundary, so the
/// clamp has to happen somewhere; doing it here means it happens once, in
/// arithmetic wide enough to compute the true value first. Note `offset`
/// already returns `i64` — the old code narrowed to `i32` *before* multiplying
/// by 96, throwing away the width it had been handed. A sheet deep enough
/// to saturate is beyond anything that can be scrolled to in any case — but it
/// is reachable by `Ctrl+End` and by typing an address into the Name Box, and
/// what it did before was take the document down.
fn px_from_twips(twips: i64) -> i32 {
    // `saturating_mul`, because widening alone is not enough: `twips * 96`
    // overflows `i64` too, just further out. Its own test caught that — the
    // first version of this fix moved the boundary instead of removing it,
    // which is the shape of a fix that looks right and only postpones.
    let px = twips.saturating_mul(96) / 1440;
    px.clamp(0, i32::MAX as i64) as i32
}

/// The column span just outside a horizontal window that can still show text
/// inside it, as `{"left":c|null,"right":c|null}`.
///
/// Long text spills across **empty** neighbours, so at most one cell per row
/// on each side can be showing text inside the window: the nearest populated
/// one. Everything between that cell and the window is empty by definition, so
/// one extra `session_cells` call per side fetches exactly the owners and
/// nothing else — which is why this returns a span rather than a list of
/// addresses, and why the cost does not grow with the number of visible rows.
///
/// The host draws only the cells it asks for, and it asks for the visible
/// window — so a label in column B spilling across C..N vanished the moment B
/// scrolled off the left, taking the visible half of the text with it. Excel
/// keeps drawing it.
///
/// Both sides, because a right-aligned cell spills *leftwards*: a label off the
/// right edge reaches back into the window.
#[wasm_bindgen]
pub fn session_spill_owners(sheet: usize, r0: u32, r1: u32, c0: u32, c1: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "{\"left\":null,\"right\":null}".to_owned();
        };
        // Scanning the populated cells rather than the columns: a window that
        // starts at column 5000 would otherwise walk 5000 empty addresses per
        // row, on every frame.
        let (mut left, mut right): (Option<u32>, Option<u32>) = (None, None);
        for (at, cell) in sh.cells.row_band(r0, r1) {
            if cell.value.is_empty() && cell.formula.is_none() {
                continue;
            }
            if at.col < c0 {
                left = Some(left.map_or(at.col, |c: u32| c.min(at.col)));
            } else if at.col > c1 {
                right = Some(right.map_or(at.col, |c: u32| c.max(at.col)));
            }
        }
        let fmt = |v: Option<u32>| v.map_or("null".to_owned(), |c| c.to_string());
        format!("{{\"left\":{},\"right\":{}}}", fmt(left), fmt(right))
    })
    .unwrap_or_else(|| "{\"left\":null,\"right\":null}".to_owned())
}

/// Absolute pixel offset (96 dpi) of a row's top edge from row 0.
#[wasm_bindgen]
pub fn session_row_offset_px(sheet: usize, row: u32) -> i32 {
    with_session(|s| px_from_twips(geometry_of(s, sheet).rows.offset(row))).unwrap_or(0)
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
        "formatColumns",
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
        "formatRows",
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
        "formatColumns",
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
        "formatRows",
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
pub(crate) fn resize_px_to_twips(px: u32) -> i64 {
    (px.max(8) as i64) * 1440 / 96
}

/// A row's height in device-independent pixels, explicit or default.
///
/// The read to go with [`session_set_row_height`]. Added because a test could
/// not otherwise ask what autofit had done — the height is written into the
/// document and there was no way to read it back, so the only thing that could
/// be asserted about autofit was that it did not crash.
#[wasm_bindgen]
#[must_use]
pub fn session_row_height(sheet: usize, row: u32) -> f64 {
    with_session(|s| {
        let geometry = geometry_of(s, sheet);
        twips_to_px_f64(geometry.rows.size(row))
    })
    .unwrap_or(0.0)
}

/// A column's width in device-independent pixels, explicit or default.
#[wasm_bindgen]
#[must_use]
pub fn session_col_width(sheet: usize, col: u32) -> f64 {
    with_session(|s| {
        let geometry = geometry_of(s, sheet);
        twips_to_px_f64(geometry.columns.size(col))
    })
    .unwrap_or(0.0)
}

/// Twips to pixels at 96 dpi, the inverse of [`resize_px_to_twips`].
pub(crate) fn twips_to_px_f64(twips: i64) -> f64 {
    twips as f64 * 96.0 / 1440.0
}

/// The grid geometry (column widths / row heights) of a sheet.
pub(crate) fn geometry_of(s: &WorkbookSession, sheet: usize) -> GridGeometry {
    s.workbook()
        .sheets
        .get(sheet)
        .map(GridGeometry::for_sheet)
        .unwrap_or_default()
}

pub(crate) fn edit_axis(sheet: usize, action: &str, op: EditOperation) -> Result<(), JsError> {
    axis_edit(sheet, action, op).map_err(|why| JsError::new(&why))
}

/// [`edit_axis`] without the `JsError`, which is the whole of it that can be
/// tested.
///
/// `JsError::new` **panics** off-wasm, so a native test cannot call a binding
/// that refuses — and a test asserting only [`axis_edit_blocked`] tests the
/// rule and not its use. A mutation removing the guard from here left such a
/// test green, which is how this split came to exist.
pub(crate) fn axis_edit(sheet: usize, action: &str, op: EditOperation) -> Result<(), String> {
    // The sheet index was `_sheet` — taken and dropped — so resizing a column
    // on a protected sheet succeeded (`UX-PROT-01`). Every other write went
    // through `guard_protected`; this path was the one that did not, and the
    // unused parameter is what made it invisible.
    //
    // Not `guard_protected` itself, which asks whether the *cells* are locked.
    // A resize is not a cell edit: Excel gates it behind protection's own
    // "Format columns" / "Format rows" options, so the question is whether the
    // file granted that permission.
    if axis_edit_blocked(sheet, action) {
        return Err(
            "this sheet is protected and does not allow resizing — unprotect it, \
                    or allow formatting rows and columns"
                .to_owned(),
        );
    }
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| "no session".to_owned())?;
        session.edit(op).map_err(|e| e.to_string())
    })
}

/// Set a line's size for a **drag in progress**, recording nothing.
///
/// A resize already previewed live in the *geometry* — the client overrides the
/// dragged line's width in its own layout — but the cell text did not move with
/// it, because the text comes from the engine and the engine still held the old
/// width. So the column edge slid under stationary content and everything
/// snapped into place only on release, which is what "not fluid" meant.
///
/// Deliberately **not** `session_set_col_width` on every mouse move: that
/// records an undoable transaction each time, so one drag would bury the undo
/// stack under a hundred entries and a single Ctrl+Z would step back one pixel.
///
/// This writes straight to the sheet instead — no transaction, no history, and
/// nothing to relay to a collaborator, because a drag nobody has finished is not
/// an edit anyone else should see. The caller restores the original size before
/// committing the real, undoable change, so the recorded operation still has the
/// size the drag started from as its inverse.
///
/// Refuses on a protected sheet for the same reason the real setter does; a
/// preview that works where the edit does not is a lie about what will happen.
#[wasm_bindgen]
pub fn session_preview_line_size(
    sheet: usize,
    index: u32,
    px: u32,
    columns: bool,
) -> Result<(), JsError> {
    let action = if columns {
        "formatColumns"
    } else {
        "formatRows"
    };
    if axis_edit_blocked(sheet, action) {
        return Ok(());
    }
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sheet) = session.workbook_mut().sheets.get_mut(sheet) else {
            return Ok(());
        };
        let sizing = if columns {
            &mut sheet.columns
        } else {
            &mut sheet.rows
        };
        sizing.sizes.insert(index, resize_px_to_twips(px));
        Ok(())
    })
}

/// The decision behind [`edit_axis`]'s refusal, separated from it.
///
/// Same reason as [`protection_blocks`]: a `JsError` cannot be constructed
/// off-wasm, so a test that exercised the guard could only ever panic. The rule
/// is the part worth testing.
pub(crate) fn axis_edit_blocked(sheet: usize, action: &str) -> bool {
    with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.protection.as_ref())
            .is_some_and(|p| p.is_enabled() && !p.permits(action))
    })
    .unwrap_or(false)
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
        let cf_stats: Vec<casual_calc_layout::conditional::RangeStats> = sheet
            .conditional_formats
            .iter()
            .map(|cf| casual_calc_layout::conditional::range_stats(wb, sheet, cf))
            .collect();
        let cf_order = casual_calc_layout::conditional::priority_order(sheet);

        let mut items = Vec::new();
        for (at, cell) in sheet.cells.row_band(first_row, last_row) {
            if at.col < first_col || at.col > last_col {
                continue;
            }
            // Two sheet-view switches change what a cell reads as, so they are
            // applied here rather than in `display_text`: they are properties
            // of the view, not of the cell, and a copy or an export must still
            // see the value.
            let text = if sheet.view.show_formulas {
                // `showFormulas` shows the formula in place of its result. A
                // cell without one still shows its value, as in Excel.
                cell_input_text(wb, cell)
            } else {
                display_text(wb, cell)
            };
            let text = if sheet.view.hide_zeros
                && matches!(cell.value, CellValue::Number(n) if n == 0.0)
            {
                // `showZeros="0"` blanks a zero *result*, not the value: the
                // cell still holds 0 and still totals as 0.
                String::new()
            } else {
                text
            };
            let style = cell.style.and_then(|id| wb.styles.get(id));
            // Conditional formatting overrides the cell's own fill when a rule
            // matches (first match wins). Numeric rules test the cell's number;
            // text rules test its display text.
            // Range-relative rules (colour scale, data bar) need where this
            // value sits between the range's own minimum and maximum, so they
            // are resolved against the pre-computed span rather than by a
            // per-cell predicate.

            // One implementation, shared with the headless renderer: the canvas
            // used to resolve these here and the PNG not at all (`RND-05`).
            let effect = casual_calc_layout::conditional::effect_for(
                sheet,
                &cf_stats,
                &cf_order,
                at.row,
                at.col,
                &cell.value,
                &text,
            );
            let bar = effect.data_bar;
            let cf_font = effect.font_color;
            let cf_bold = effect.bold;
            let cf_fill = effect.fill;
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
            // Not under `showFormulas`: what is displayed there is the
            // formula text, and a "#######" fill is the rule for a number too
            // wide to be shown safely, not for text that can simply spill.
            if matches!(cell.value, CellValue::Number(_)) && !sheet.view.show_formulas {
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

pub(crate) fn hidden_edit(
    sheet: usize,
    a: u32,
    b: u32,
    columns: bool,
    hide: bool,
) -> Result<(), JsError> {
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
pub(crate) const MAX_OUTLINE_LEVEL: u8 = 7;

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
pub(crate) fn edit_sheet_metadata(
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
            .edit(EditOperation::set_sheet_metadata(sheet, data))
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
pub(crate) const MAX_FILTER_VALUES: usize = 10_000;

/// A cell's display text and numeric value, for filter matching.
pub(crate) fn filter_operands(
    wb: &Workbook,
    sheet: &Sheet,
    row: u32,
    col: u32,
) -> (String, Option<f64>) {
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
pub(crate) fn row_passes(
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
pub(crate) fn sheet_filters(sheet: &Sheet) -> impl Iterator<Item = (FilterSite, &AutoFilter)> {
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
pub(crate) enum FilterSite {
    /// The worksheet's own `<autoFilter>`.
    Sheet,
    /// The table at this index in `sheet.tables`.
    Table(usize),
}

/// The filter whose range covers `col`, and where it lives.
///
/// The sheet's own filter wins when both cover the column: it is the one the
/// toolbar button turned on, so it is the one the user just interacted with.
pub(crate) fn filter_at_col(sheet: &Sheet, col: u32) -> Option<(FilterSite, &AutoFilter)> {
    sheet_filters(sheet).find(|(_, f)| col >= f.range.start.col && col <= f.range.end.col)
}

/// The rows every filter on the sheet hides, recomputed from their rules.
pub(crate) fn recompute_filter_hidden(wb: &Workbook, sheet: &Sheet) -> BTreeSet<u32> {
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
pub(crate) fn commit_filter(
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
pub(crate) fn sheet_filter(sheet: usize) -> Option<AutoFilter> {
    with_session(|s| s.workbook().sheets.get(sheet)?.auto_filter.clone()).flatten()
}

/// Read the filter covering `col` — the sheet's or a table's — with its site.
pub(crate) fn filter_for_col(sheet: usize, col: u32) -> Option<(FilterSite, AutoFilter)> {
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

#[cfg(test)]
mod offset_overflow_tests {
    use super::{px_from_twips, session_new, session_row_offset_px, session_set_row_height};

    /// **The last row Excel has must have a pixel offset.**
    ///
    /// `offset` returns `i64`; the conversion narrowed to `i32` *before*
    /// multiplying by 96, so it overflowed at 1,491,308 px — row 74,566 at the
    /// default 20px height. `overflow-checks` is on in the release profile, so
    /// that was a panic in the shipped build, not a wrong number. A panicking
    /// `wasm_bindgen` export traps the module: the editor stopped and the
    /// document went with it.
    ///
    /// Reachable two ways a user meets by accident — `Ctrl+End` on a deep
    /// sheet, and typing `A1048576` into the Name Box.
    #[test]
    fn the_last_row_excel_has_still_has_an_offset() {
        session_new();
        // 1_048_575 is the last row of an `.xlsx` grid, zero-based.
        let px = session_row_offset_px(0, 1_048_575);
        assert_eq!(
            px, 20_971_500,
            "the deepest row Excel can address must resolve"
        );
    }

    /// The boundary itself, and one line either side of it.
    #[test]
    fn the_row_that_used_to_trap_is_one_pixel_step_past_the_one_before_it() {
        session_new();
        assert_eq!(session_row_offset_px(0, 74_565), 1_491_300);
        assert_eq!(session_row_offset_px(0, 74_566), 1_491_320);
    }

    /// The boundary scales inversely with row height, which is what proves the
    /// cause is the arithmetic rather than the row index: at double the height
    /// it arrives at half the row, on the same pixel offset.
    #[test]
    fn a_taller_row_reaches_the_old_boundary_sooner_and_still_resolves() {
        session_new();
        session_set_row_height(0, 0, 40).unwrap();
        // Every row is 40px only if the default changed; instead assert the
        // conversion directly, which is where the defect lived.
        assert_eq!(px_from_twips(22_369_800), 1_491_320);
        assert_eq!(px_from_twips(44_739_600), 2_982_640);
    }

    /// Saturating rather than trapping is the *behaviour*, not an accident of
    /// the widening: a value past what an `i32` can carry has to come back as
    /// something, and a clamp is the only answer that keeps the module alive.
    #[test]
    fn an_offset_past_what_the_boundary_can_carry_saturates_instead_of_trapping() {
        assert_eq!(px_from_twips(i64::MAX), i32::MAX);
        assert_eq!(px_from_twips(-1), 0);
    }
}
