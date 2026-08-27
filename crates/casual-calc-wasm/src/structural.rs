//! Operations that move cells: inserting and deleting bands, merging,
//! shifting, sorting and de-duplicating.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// Whether a merge intersects the box `[r0,c0]..[r1,c1]`.
pub(crate) fn merge_hits(
    m: &casual_calc_model::CellRange,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> bool {
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
    guard_protected(sheet, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))?;
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
    // The destructive form, which throws away everything but the top-left
    // value — so if either entry point must obey protection, it is this one.
    guard_protected(sheet, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        merge_discarding(session, sheet, r0, c0, r1, c1)
    })
}

/// The body of [`session_merge_cells_discarding`], against a session the caller
/// already holds.
///
/// Extracted so a paste carrying `rowspan`/`colspan` merges exactly the way the
/// menu command does — two implementations of "merge, keeping the block's
/// styling" would be two chances to disagree about what a merge clears.
pub(crate) fn merge_discarding(
    session: &mut WorkbookSession,
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    {
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
    }
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

/// Delete `count` rows starting at `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_delete_rows(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    guard_protected(sheet, at, 0, at.saturating_add(count.max(1) - 1), 0)?;
    commit_edit(EditOperation::DeleteRows { sheet, at, count })
}

/// Insert `count` blank columns before `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_insert_columns(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    guard_protected(sheet, 0, at, 0, at.saturating_add(count.max(1) - 1))?;
    commit_edit(EditOperation::InsertColumns { sheet, at, count })
}

/// Delete `count` columns starting at `at` (undoable; rewrites formula refs).
#[wasm_bindgen]
pub fn session_delete_columns(sheet: usize, at: u32, count: u32) -> Result<(), JsError> {
    guard_protected(sheet, 0, at, 0, at.saturating_add(count.max(1) - 1))?;
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

// --- Named cell styles ----------------------------------------------------

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
                        // **In and out of the absolute form**, as the
                        // structural rewrite does: `sort_reanchor` asks which
                        // references sit *on* the moving row, which is a
                        // question about addresses. The row's cells keep their
                        // column, so only the row of the origin changes.
                        let from = Origin::at(row.src_row, c);
                        let to = Origin::at(r, c);
                        let absolute = restore_at(expr, from, ABSOLUTE);
                        let shifted = restore_at(
                            &sort_reanchor(&absolute, dr, row.src_row, c0, c1),
                            ABSOLUTE,
                            to,
                        );
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
        // Record what was sorted. Excel writes `<sortState>` so its own dialog
        // can reopen showing the keys that were used; sorting here left no
        // trace, so a file sorted in this app opened in Excel claiming it had
        // never been sorted. Part of the same undo step as the move.
        let mut sort = casual_calc_model::SortState::default();
        sort.attrs.insert(
            "ref".to_owned(),
            format!(
                "{}{}:{}{}",
                casual_calc_formula::column_to_letters(c0),
                r0 + 1,
                casual_calc_formula::column_to_letters(c1),
                r1 + 1
            ),
        );
        for (i, &key) in key_cols.iter().enumerate() {
            let mut cond: std::collections::BTreeMap<String, String> = Default::default();
            cond.insert(
                "ref".to_owned(),
                format!(
                    "{}{}:{}{}",
                    casual_calc_formula::column_to_letters(key),
                    r0 + 1,
                    casual_calc_formula::column_to_letters(key),
                    r1 + 1
                ),
            );
            // `descending` is the attribute; ascending is the schema default
            // and is written by leaving it out.
            if ascending.get(i).copied().unwrap_or(1) == 0 {
                cond.insert("descending".to_owned(), "1".to_owned());
            }
            sort.conditions.push(cond);
        }
        if let Some(sh) = session.workbook().sheets.get(sheet).cloned() {
            let mut data = SheetMetadata::capture(&sh);
            data.sort_state = Some(sort);
            ops.push(EditOperation::set_sheet_metadata(sheet, data));
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Whether a reference moves with a sorted row: relative, unqualified, on the
/// source row, and inside the sorted columns.
pub(crate) fn ref_moves_with_row(r: &StoredRef, src_row: u32, c0: u32, c1: u32) -> bool {
    // Addresses: reached with a tree in the absolute form, where a stored
    // reference's offset from `(0, 0)` is the address it names.
    !r.row_absolute
        && r.sheet.is_none()
        && r.row == i64::from(src_row)
        && r.col >= i64::from(c0)
        && r.col <= i64::from(c1)
}

pub(crate) fn shifted_row(r: &StoredRef, dr: i64) -> StoredRef {
    let mut out = r.clone();
    out.row = (r.row + dr).max(0);
    out
}

/// Re-anchor a formula for a row moved by `dr` during a sort: shift only the
/// references that travel with the row (see [`ref_moves_with_row`]); a range is
/// shifted only when both endpoints do, so a multi-row range is never split.
pub(crate) fn sort_reanchor(expr: &Expr, dr: i64, src_row: u32, c0: u32, c1: u32) -> Expr {
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
