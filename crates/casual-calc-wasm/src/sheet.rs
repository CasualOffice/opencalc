//! Sheets themselves: adding, renaming, ordering, visibility and protection.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

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
pub(crate) fn ci_replace(haystack: &str, needle: &str, repl: &str) -> String {
    if needle.is_empty() {
        return haystack.to_owned();
    }
    let needle_l = needle.to_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut rest = haystack;
    while !rest.is_empty() {
        // **Lowercased per position, from the original.** The previous version
        // lowercased the whole haystack once and then indexed it with offsets
        // taken from the *original* — which is only sound if lowercasing
        // preserves byte length, and it does not. `İ` (U+0130) lowercases to
        // two characters and `K` (U+212A, the Kelvin sign) lowercases to one
        // byte, so the two strings drift apart and `hay_l[i..]` slices out of
        // bounds or into the middle of a character.
        //
        // Either one panics, and a panic here is not an error the caller can
        // handle: on wasm32 it is a trap that aborts the module, leaves the
        // `RefCell` borrow held across it permanently locked, and takes the
        // open workbook with it. An ordinary Turkish column heading was enough,
        // through the default Find bar path — "match case" is off by default.
        // Lowercase forward from this position one character at a time until
        // enough bytes have been produced to decide, tracking how much of the
        // *original* was consumed as we go. No index ever crosses between the
        // two encodings, so no length change can put one out of step.
        let mut consumed = 0usize;
        let mut lowered = String::new();
        for ch in rest.chars() {
            consumed += ch.len_utf8();
            lowered.extend(ch.to_lowercase());
            if lowered.len() >= needle_l.len() {
                break;
            }
        }
        if lowered == needle_l {
            out.push_str(repl);
            rest = &rest[consumed..];
        } else {
            let ch = rest.chars().next().expect("rest is not empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

pub(crate) fn contains_ci(hay: &str, needle: &str, match_case: bool) -> bool {
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

/// Set `locked` / `formula_hidden` on a range (one undo step).
///
/// `which` is `locked` or `hidden`. Both only bite while the sheet is
/// protected, which is why the menu says so — a checkbox that appears to do
/// nothing is worse than no checkbox.
#[wasm_bindgen]
pub fn session_set_cell_protection(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    which: &str,
    on: bool,
) -> Result<(), JsError> {
    let which = which.to_owned();
    apply_style_range(sheet, r0, c0, r1, c1, move |st| match which.as_str() {
        // `locked` defaults to true in OOXML, so "locked" is written as `None`
        // and only the unlocked case carries an attribute. That keeps an
        // untouched workbook byte-identical.
        "locked" => st.locked = (!on).then_some(false),
        "hidden" => st.formula_hidden = on.then_some(true),
        _ => {}
    })
}

/// Whether every cell in a range is locked / formula-hidden, as JSON
/// `{locked, hidden}` — what the menu ticks.
#[wasm_bindgen]
pub fn session_cell_protection(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let style = wb
            .sheets
            .get(sheet)
            .and_then(|sh| sh.cells.get(CellRef::new(row, col)))
            .and_then(|c| c.style)
            .and_then(|id| wb.styles.get(id));
        format!(
            "{{\"locked\":{},\"hidden\":{}}}",
            style.and_then(|st| st.locked).unwrap_or(true),
            style.and_then(|st| st.formula_hidden).unwrap_or(false)
        )
    })
    .unwrap_or_else(|| "{\"locked\":true,\"hidden\":false}".to_owned())
}

/// A sheet's page setup as JSON, flattened to `group.attribute` keys.
///
/// Every one of these was read, written and carried verbatim with nothing able
/// to change it: a sheet imported as landscape could only ever be saved as
/// landscape. The groups are the OOXML elements — `page` is `<pageSetup>`,
/// `margins` is `<pageMargins>`, `options` is `<printOptions>`, `setupPr` is
/// `<pageSetUpPr>`, and `hf` is `<headerFooter>`'s child *text*.
#[wasm_bindgen]
pub fn session_page_setup(sheet: usize) -> String {
    with_session(|s| {
        let p = &s.workbook().sheets.get(sheet)?.print;
        let mut items: Vec<String> = Vec::new();
        let mut push = |group: &str, map: &std::collections::BTreeMap<String, String>| {
            for (k, v) in map {
                items.push(format!(
                    "{}:{}",
                    json_string(&format!("{group}.{k}")),
                    json_string(v)
                ));
            }
        };
        push("page", &p.page);
        push("margins", &p.margins);
        push("options", &p.options);
        push("setupPr", &p.setup_pr);
        push("hf", &p.header_footer_text);
        Some(format!("{{{}}}", items.join(",")))
    })
    .flatten()
    .unwrap_or_else(|| "{}".to_owned())
}

/// The sheet the file was left open on — OOXML's `tabSelected` — or 0.
///
/// A workbook remembers which tab its author was looking at, and opening every
/// file on the first sheet ignores that: a summary sheet at the end is the
/// whole point of putting it there. A hidden sheet is skipped, because a tab
/// that cannot be shown cannot be the one to open on.
#[wasm_bindgen]
pub fn session_active_sheet() -> usize {
    with_session(|s| {
        s.workbook()
            .sheets
            .iter()
            .position(|sh| {
                sh.view.tab_selected && sh.visibility == casual_calc_model::SheetVisibility::Visible
            })
            .unwrap_or(0)
    })
    .unwrap_or(0)
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

/// How a data bar is drawn, so the canvas need not decide for itself.
///
/// The editor paints its own data bars — there is no display list across this
/// boundary, and building one is a much larger change than `RND-08` reads as.
/// What that row is really about is the two renderers being able to drift
/// apart, and they could: the inset, the alpha and the default colour were
/// written out twice and agreed only because somebody had copied them.
///
/// This makes `casual-calc-render` the one place they are decided. The canvas
/// still does its own painting; it no longer does its own deciding.
#[wasm_bindgen]
#[must_use]
pub fn session_data_bar_style() -> String {
    let style = casual_calc_render::data_bar_style();
    format!(
        r#"{{"padX":{},"padY":{},"alpha":{},"defaultColor":"{}"}}"#,
        style.pad_x, style.pad_y, style.alpha, style.default_color
    )
}
