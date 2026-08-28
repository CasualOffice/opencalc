//! Copy, cut and paste — the internal clip and the OS clipboard's TSV and
//! HTML flavours.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

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
    // The destination is what gets written. `session_fill` delegates here, so
    // this one guard covers Ctrl+D, Ctrl+R and the fill handle alike.
    guard_protected(
        sheet,
        dr0.min(dr1),
        dc0.min(dc1),
        dr0.max(dr1),
        dc0.max(dc1),
    )?;
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
            // A date is a number wearing a format, and that format is the only
            // thing that tells one from a plain number here.
            let date_lit = |r: u32, c: u32| -> bool {
                sh.cells.get(CellRef::new(r, c)).is_some_and(|cell| {
                    matches!(cell.value, CellValue::Number(_))
                        && cell.formula.is_none()
                        && casual_calc_layout::cell_number_format(wb, cell)
                            .is_some_and(is_day_format)
                })
            };
            // If the fill grows along exactly one axis and each line of the
            // source is a numeric arithmetic sequence (>=2 cells, constant
            // step), extend the sequence instead of tiling — Excel's autofill.
            let vertical = dc0 == sc0 && dc1 == sc1 && (dr1 > sr1 || dr0 < sr0);
            let horizontal = dr0 == sr0 && dr1 == sr1 && (dc1 > sc1 || dc0 < sc0);
            let growth = mode == "growth";
            // `steps_alone` — whether a *single* source cell already implies a
            // step of one. Excel's asymmetry: dragging one `5` gives `5 5 5`,
            // dragging one `2024-01-01` gives consecutive days.
            let arithmetic = |vals: &[Option<f64>], steps_alone: bool| -> Option<(f64, f64)> {
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
                    // An explicit "fill series" steps by one from a single cell,
                    // and so does a date — one day. For a plain number, auto
                    // detection needs two cells to know the step, and one cell
                    // copies.
                    return (mode == "series" || steps_alone).then(|| (vals[0].unwrap(), 1.0));
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
                    .map(|c| {
                        arithmetic(
                            &(sr0..=sr1).map(|r| num_lit(r, c)).collect::<Vec<_>>(),
                            sr0 == sr1 && date_lit(sr0, c),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let row_series: Vec<Option<(f64, f64)>> = if horizontal {
                (sr0..=sr1)
                    .map(|r| {
                        arithmetic(
                            &(sc0..=sc1).map(|c| num_lit(r, c)).collect::<Vec<_>>(),
                            sc0 == sc1 && date_lit(r, sc0),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            // Text series, per line, alongside the numeric: a named list
            // (month/weekday) or a trailing count (`Item 1 → Item 2`).
            let text_fill = |vals: &[Option<String>]| -> Option<TextFill> {
                // Copy never extends, whatever the values look like — the same
                // rule the numeric path keeps.
                if mode == "copy" || mode == "formats" {
                    return None;
                }
                detect_text_fill(vals)
            };
            let col_text: Vec<Option<TextFill>> = if vertical {
                (sc0..=sc1)
                    .map(|c| text_fill(&(sr0..=sr1).map(|r| text_lit(r, c)).collect::<Vec<_>>()))
                    .collect()
            } else {
                Vec::new()
            };
            let row_text: Vec<Option<TextFill>> = if horizontal {
                (sr0..=sr1)
                    .map(|r| text_fill(&(sc0..=sc1).map(|c| text_lit(r, c)).collect::<Vec<_>>()))
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
                    // Text series (named list or trailing count) along the axis.
                    let text_series = if vertical {
                        col_text[(dc - sc0) as usize]
                            .as_ref()
                            .and_then(|f| text_fill_at(f, dr as i64 - sr0 as i64))
                    } else if horizontal {
                        row_text[(dr - sr0) as usize]
                            .as_ref()
                            .and_then(|f| text_fill_at(f, dc as i64 - sc0 as i64))
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
                            let formula = c.formula.and_then(|h| wb.formula(h)).cloned();
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
pub(crate) fn html_cell_css(style: &Style) -> String {
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
    // **Validated, not escaped.** A colour comes out of the file's `styles.xml`
    // verbatim — the importer preserves whatever the attribute said, as it must
    // (docs/34) — and this string is dropped into a `style="…"` attribute that
    // `session_print_html` hands to `document.write` in a window inheriting the
    // editor's origin. A workbook whose `<color rgb>` closed the attribute and
    // opened an `<img onerror=…>` therefore ran script with a live
    // `window.opener`, next to the session token and the collaboration socket.
    //
    // Escaping would work and is the wrong tool: there is no legitimate colour
    // that needs escaping. A colour is hex or it is not a colour, and one that
    // is not is dropped — the cell renders in the default colour rather than
    // carrying an attacker's string into a document. Doing it here rather than
    // at import also covers every emitter that formats a colour, and the CI
    // SEC-001 sink check cannot see this file at all: it greps `webapp/*.js`
    // and the host's HTML, so markup assembled in Rust was outside it.
    let hex = |c: &String| -> Option<String> {
        let ok = matches!(c.len(), 3 | 6 | 8) && c.chars().all(|ch| ch.is_ascii_hexdigit());
        ok.then(|| c.clone())
    };
    if let Some(c) = style.font_color.as_ref().and_then(hex) {
        css.push_str(&format!("color:#{c};"));
    }
    if let Some(c) = style.fill_color.as_ref().and_then(hex) {
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

pub(crate) fn push_html_escaped(out: &mut String, text: &str) {
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
    // Measured rather than assumed. The comment here used to say the extent
    // was unknowable before parsing and that the anchor was therefore enough —
    // but the parse is a split, it costs nothing to do first, and a one-cell
    // guard let a multi-row paste land on locked cells.
    let (mut rows, mut cols) = (0u32, 0u32);
    for (dr, line) in tsv.split('\n').enumerate() {
        if line.is_empty() && dr > 0 {
            continue;
        }
        rows = rows.max(dr as u32);
        cols = cols.max(line.split('\t').count().max(1) as u32 - 1);
    }
    guard_protected(
        sheet,
        row,
        col,
        row.saturating_add(rows),
        col.saturating_add(cols),
    )?;
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
                ops.push(session.input_edit(sheet, at, field));
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// One cell of a parsed clipboard table.
///
/// The wire between the browser's HTML parser and the engine
/// ([68](../../../docs/68-CLIPBOARD-HTML-PASTE.md)). Deliberately named
/// properties rather than markup or CSS text: nothing that crosses this boundary
/// can carry a script, a URL or a selector, so there is no sanitising to get
/// wrong on this side.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PastedCell {
    /// Row offset from the paste anchor.
    dr: u32,
    /// Column offset from the paste anchor.
    dc: u32,
    /// Rows this cell spans (`rowspan`), 1 when it spans only itself.
    rs: u32,
    /// Columns this cell spans (`colspan`).
    cs: u32,
    /// The cell's displayed text, already unescaped by the HTML parser.
    text: String,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    wrap: bool,
    /// Six hex digits, no `#`. Anything the parser could not normalise is absent
    /// rather than guessed.
    color: Option<String>,
    fill: Option<String>,
    font: Option<String>,
    /// Font size in half-points, as the model stores it.
    size_hp: Option<u32>,
    /// `left` | `center` | `right` | `justify`.
    align: Option<String>,
    /// `top` | `middle` | `bottom`.
    valign: Option<String>,
    /// A number-format code, from Excel's `mso-number-format` or LibreOffice's
    /// `sdnum`. Absent for producers that emit neither.
    number_format: Option<String>,
    /// The edges the cell declared for itself. An absent edge is "no opinion",
    /// not "no line" — the same rule every other field here follows.
    borders: Option<PastedBorders>,
}

/// The four edges of one pasted cell.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PastedBorders {
    top: Option<PastedEdge>,
    right: Option<PastedEdge>,
    bottom: Option<PastedEdge>,
    left: Option<PastedEdge>,
}

/// One edge, already reduced to an OOXML line-style token by the parser.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PastedEdge {
    style: String,
    color: Option<String>,
}

impl PastedEdge {
    /// Every line style OOXML defines.
    const STYLES: [&'static str; 14] = [
        "thin",
        "medium",
        "thick",
        "double",
        "dashed",
        "dotted",
        "hair",
        "dashDot",
        "dashDotDot",
        "mediumDashed",
        "mediumDashDot",
        "mediumDashDotDot",
        "slantDashDot",
        "none",
    ];

    /// The model edge, or `None` when the token is not one OOXML defines.
    ///
    /// The parser only ever emits from a fixed set, so this cannot currently
    /// reject anything. It is here because `style` is written into the file
    /// verbatim: were this string ever to reach the writer unchecked, a
    /// clipboard could put arbitrary text inside `<border style="…">` and
    /// produce a package Excel refuses to open. Validating at the boundary
    /// costs a lookup and removes the question.
    fn edge(&self) -> Option<casual_calc_model::BorderEdge> {
        if !Self::STYLES.contains(&self.style.as_str()) || self.style == "none" {
            return None;
        }
        Some(casual_calc_model::BorderEdge {
            style: self.style.clone(),
            color: self.color.clone(),
        })
    }
}

/// Paste a parsed clipboard table: values, spans and the styles that survive
/// the clipboard, as **one** transaction.
///
/// One `Batch` because a paste is one thing a person did: one undo takes all of
/// it back, and a collaborator receives it as a unit rather than watching a
/// grid fill in cell by cell.
///
/// The markup itself never reaches here — see
/// [68](../../../docs/68-CLIPBOARD-HTML-PASTE.md) for why the browser parses it
/// and what that costs.
#[wasm_bindgen]
pub fn session_paste_html(sheet: usize, row: u32, col: u32, cells: &str) -> Result<(), JsError> {
    let parsed: Vec<PastedCell> =
        serde_json::from_str(cells).map_err(|why| JsError::new(&format!("bad paste: {why}")))?;
    if parsed.is_empty() {
        return Ok(());
    }
    // Over the whole block. `parsed` already carries every `dr`/`dc` and span
    // one line above this, so guarding a 1x1 anchor was a choice rather than a
    // limitation — and it let a paste from Excel overwrite ten locked rows
    // while the status bar said `pasted`.
    let (mut last_row, mut last_col) = (0u32, 0u32);
    for c in &parsed {
        last_row = last_row.max(c.dr.saturating_add(c.rs.max(1) - 1));
        last_col = last_col.max(c.dc.saturating_add(c.cs.max(1) - 1));
    }
    guard_protected(
        sheet,
        row,
        col,
        row.saturating_add(last_row),
        col.saturating_add(last_col),
    )?;

    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let mut ops = Vec::new();
        let mut merges: Vec<(u32, u32, u32, u32)> = Vec::new();

        for c in &parsed {
            let at = CellRef::new(row.saturating_add(c.dr), col.saturating_add(c.dc));
            ops.push(session.input_edit(sheet, at, &c.text));

            // Styles are applied over whatever the target cell already had, the
            // same way `session_set_style` does: a paste carrying no opinion
            // about italics must not silently clear the italics already there.
            let mut style = session
                .workbook()
                .sheets
                .get(sheet)
                .and_then(|s| s.cells.get(at))
                .and_then(|cell| cell.style)
                .and_then(|id| session.workbook().styles.get(id))
                .cloned()
                .unwrap_or_default();
            let mut touched = false;
            if c.bold {
                style.bold = true;
                touched = true;
            }
            if c.italic {
                style.italic = true;
                touched = true;
            }
            if c.underline {
                style.underline = Some(casual_calc_model::Underline::Single);
                touched = true;
            }
            if c.strike {
                style.strike = true;
                touched = true;
            }
            if c.wrap {
                style.wrap = true;
                touched = true;
            }
            if let Some(hex) = c.color.as_ref() {
                style.font_color = Some(hex.clone());
                touched = true;
            }
            if let Some(hex) = c.fill.as_ref() {
                style.fill_color = Some(hex.clone());
                touched = true;
            }
            if let Some(name) = c.font.as_ref() {
                style.font_name = Some(name.clone());
                touched = true;
            }
            if let Some(hp) = c.size_hp {
                style.font_size_hp = Some(hp);
                touched = true;
            }
            if let Some(a) = c.align.as_deref() {
                style.align = match a {
                    "left" => Some(casual_calc_model::HAlign::Left),
                    "center" => Some(casual_calc_model::HAlign::Center),
                    "right" => Some(casual_calc_model::HAlign::Right),
                    "justify" => Some(casual_calc_model::HAlign::Justify),
                    _ => style.align,
                };
                touched = true;
            }
            if let Some(a) = c.valign.as_deref() {
                style.valign = match a {
                    "top" => Some(casual_calc_model::VAlign::Top),
                    "middle" => Some(casual_calc_model::VAlign::Middle),
                    "bottom" => Some(casual_calc_model::VAlign::Bottom),
                    _ => style.valign,
                };
                touched = true;
            }
            if let Some(code) = c.number_format.as_ref() {
                style.number_format = Some(code.clone());
                touched = true;
            }
            if let Some(edges) = c.borders.as_ref() {
                let mut border = style.border.clone().unwrap_or_default();
                // Only the edges the clipboard declared are set. An edge it said
                // nothing about keeps whatever the target cell already had, so
                // pasting a bottom rule into a boxed cell does not strip the box.
                let mut any = false;
                for (declared, slot) in [
                    (&edges.top, &mut border.top),
                    (&edges.right, &mut border.right),
                    (&edges.bottom, &mut border.bottom),
                    (&edges.left, &mut border.left),
                ] {
                    if let Some(edge) = declared.as_ref().and_then(PastedEdge::edge) {
                        *slot = Some(edge);
                        any = true;
                    }
                }
                if any {
                    style.border = Some(border);
                    touched = true;
                }
            }
            if touched {
                let id = session.workbook_mut().intern_style(style);
                ops.push(EditOperation::SetStyle {
                    sheet,
                    at,
                    style: Some(id),
                });
            }

            if c.rs > 1 || c.cs > 1 {
                merges.push((
                    at.row,
                    at.col,
                    at.row + c.rs.max(1) - 1,
                    at.col + c.cs.max(1) - 1,
                ));
            }
        }

        session.edit(EditOperation::Batch(ops)).map_err(js)?;
        // Merges are sheet metadata rather than cell operations, so they cannot
        // ride in the same batch. Applied after, so the values are already in
        // place and a merge never covers a cell this paste has yet to write.
        for (r0, c0, r1, c1) in merges {
            merge_discarding(session, sheet, r0, c0, r1, c1)?;
        }
        Ok(())
    })
}

/// A cell captured on the internal clipboard. `dr`/`dc` are the cell's offset
/// among the **visible** cells of the copied range (hidden rows/columns are
/// skipped and the rest compressed), so a paste lands them contiguously.
/// `sr`/`sc` keep the original address for cut-clearing and per-cell formula
/// reference shifting.
pub(crate) struct ClipCell {
    pub(crate) dr: u32,
    pub(crate) dc: u32,
    pub(crate) sr: u32,
    pub(crate) sc: u32,
    pub(crate) cell: Cell,
    pub(crate) formula: Option<Expr>,
}

/// The internal (rich) clipboard: keeps values, styles, and resolved formula
/// ASTs so a paste can reproduce formulas (reference-shifted) and formatting —
/// unlike the text-only OS clipboard.
pub(crate) struct Clip {
    sheet: usize,
    cut: bool,
    cells: Vec<ClipCell>,
    /// The source columns' widths in twips, by offset from the copied range's
    /// first column, for a paste that asks for them (`UX-CLIP-02`).
    ///
    /// Captured on copy rather than read on paste, because by then the source
    /// may have been resized — or, after a cut, may not be there at all. Only
    /// columns with an *explicit* width are listed: a column at the sheet
    /// default has no width to carry, and writing the default onto the
    /// destination would silently pin a column that was following it.
    widths: Vec<(u32, i64)>,
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
pub(crate) fn clip_capture(
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
        let widths = (c0..=c1)
            .filter(|c| !sh.hidden_cols.contains(c))
            .enumerate()
            .filter_map(|(offset, c)| {
                sh.columns
                    .sizes
                    .get(&c)
                    .map(|w| (u32::try_from(offset).unwrap_or(0), *w))
            })
            .collect();
        CLIP.with(|cl| {
            *cl.borrow_mut() = Some(Clip {
                sheet,
                cut,
                cells,
                widths,
            });
        });
    });
}

/// Whether the internal clipboard currently holds a snapshot.
/// Forget what is on the clipboard.
///
/// Esc after a cut has to reach the engine, not merely stop the marching ants.
/// Cancelling the animation alone left the pending cut armed, so Esc, a click
/// elsewhere, and Ctrl+V still **moved** the data and emptied the source the
/// user believed they had spared — the visible signal said cancelled and the
/// state said otherwise.
#[wasm_bindgen]
pub fn session_clip_clear() {
    CLIP.with(|cl| *cl.borrow_mut() = None);
}

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
/// formatting), `"transpose"` (a full paste with rows and columns swapped), or
/// `"widths"` (the source columns' widths and nothing else — `UX-CLIP-02`).
/// A cut only takes effect for `"all"` (Excel disables cut with paste-special).
#[wasm_bindgen]
pub fn session_clip_paste_mode(
    sheet: usize,
    row: u32,
    col: u32,
    mode: &str,
) -> Result<(), JsError> {
    // Guarded over the paste's whole extent, not its anchor. The extent is
    // knowable here — it is the clipboard's own span, and CLIP is a separate
    // thread-local, so it can be measured before SESSION is borrowed.
    let transpose = mode == "transpose";
    let span = CLIP.with(|cl| {
        cl.borrow().as_ref().map(|clip| {
            let (mut dr, mut dc) = (0, 0);
            for c in &clip.cells {
                dr = dr.max(c.dr);
                dc = dc.max(c.dc);
            }
            if transpose { (dc, dr) } else { (dr, dc) }
        })
    });
    // The anchor is guarded even when the clipboard is empty. Pasting nothing
    // changes nothing, so letting it through would be harmless — but the user
    // pressed Ctrl+V on a protected sheet and is owed the same answer either
    // way, rather than silence that depends on what they happened to copy last.
    let (dr, dc) = span.unwrap_or((0, 0));
    guard_protected(
        sheet,
        row,
        col,
        row.saturating_add(dr),
        col.saturating_add(dc),
    )?;
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let (ops, was_cut, empty) = CLIP.with(|cl| {
            let borrow = cl.borrow();
            let Some(clip) = borrow.as_ref() else {
                return (Vec::new(), false, true);
            };
            let cut = clip.cut && mode == "all";
            let mut ops = Vec::new();

            // **Column widths, and nothing else** (`UX-CLIP-02`). Excel's
            // Paste Special has this as its own option because a plain paste
            // must not reshape the sheet it lands in — a person pasting three
            // cells does not expect their columns to move. Asked for
            // explicitly, it is exactly what they want.
            //
            // Widths only. A row's height is a property of the row, and a
            // paste that also set those would resize rows the copied block
            // merely passed through.
            if mode == "widths" {
                for (offset, twips) in &clip.widths {
                    ops.push(EditOperation::SetColumnWidth {
                        sheet,
                        col: col.saturating_add(*offset),
                        width: Some(*twips),
                    });
                }
                return (ops, false, clip.widths.is_empty());
            }
            // Where the block landed, learned from the first cell placed. A cut
            // is never tiled or transposed, so one delta describes the move.
            let mut move_delta: Option<(i64, i64)> = None;
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
                            // `continue`, not `return`. This used to leave the
                            // whole closure on the first non-numeric source, so
                            // a column of figures with one heading in it pasted
                            // as far as the heading and stopped — the cells
                            // above updated, the ones below silently did not,
                            // and the status bar said `pasted add` regardless.
                            continue;
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
                            // The target is text or an error: leave it alone
                            // and carry on, for the same reason as the source
                            // above. Skipping a cell is what the comment three
                            // lines up already promised.
                            _ => continue,
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
                            // **No shift.** The tree is stored relative to the
                            // cell it came from, so storing it at the
                            // destination *is* the shift — `=A1+1` a column
                            // over reads `=B1+1` because it is read there.
                            out.formula = Some(session.workbook_mut().store_formula(expr.clone()));
                        }
                        ops.push(EditOperation::SetCell {
                            sheet,
                            at,
                            cell: Some(out),
                        });
                    }
                    _ => {
                        let mut out = cc.cell.clone();
                        // A **copy** shifts references by the per-cell delta —
                        // that is what makes `=A1+1` become `=B1+1` a column
                        // over. A **cut moves the cell**, so its formula travels
                        // verbatim: `=A1+1` cut from B1 to D5 is still `=A1+1`
                        // in Excel, because the cell did not change what it
                        // means, only where it lives.
                        //
                        // **Since `PERF-11` the two have swapped which one does
                        // the work.** A stored tree is relative to its cell, so
                        // a copy shifts by being read at the destination and
                        // needs nothing done to it; a cut has to be re-stored,
                        // or its offsets would measure from the new cell and
                        // point somewhere it never referred to.
                        if cut && move_delta.is_none() {
                            move_delta =
                                Some((at.row as i64 - cc.sr as i64, at.col as i64 - cc.sc as i64));
                        }
                        if let Some(expr) = &cc.formula {
                            let travelled = if cut {
                                restore_at(
                                    expr,
                                    Origin::at(cc.sr, cc.sc),
                                    Origin::at(at.row, at.col),
                                )
                            } else {
                                expr.clone()
                            };
                            out.formula = Some(session.workbook_mut().store_formula(travelled));
                        }
                        ops.push(EditOperation::SetCell {
                            sheet,
                            at,
                            cell: Some(out),
                        });
                    }
                }
            }
            // **A cut moves the cells, so everything pointing at them
            // follows.** The block itself travels verbatim, which is right --
            // the cell did not change what it means. But every *other* formula
            // that named those cells kept its old address and silently began
            // reading whatever moved in underneath (`UX-CUT-03`). Excel
            // repoints them, and it must happen inside this same batch so the
            // move is one undo step rather than two.
            //
            // Scoped to formulas *outside* the block: a reference from inside
            // the block to another cell inside it is the verbatim-travel case
            // above, and resurrecting a source cell this batch is about to
            // clear would be worse than the defect.
            if let Some((dr, dc)) = move_delta
                && (dr != 0 || dc != 0)
            {
                let sheet_name = session
                    .workbook()
                    .sheets
                    .get(clip.sheet)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                let block = clip.cells.iter().fold(
                    (u32::MAX, u32::MAX, 0u32, 0u32),
                    |(r0, c0, r1, c1), cc| {
                        (r0.min(cc.sr), c0.min(cc.sc), r1.max(cc.sr), c1.max(cc.sc))
                    },
                );
                let repointed = casual_calc_transaction::repointed_after_move(
                    session.workbook(),
                    &sheet_name,
                    block,
                    (dr, dc),
                );
                // Ahead of the paste, so a repointed cell the paste also lands
                // on keeps the pasted content rather than the rewrite.
                let mut front = Vec::new();
                for (sheet, at, expr) in repointed {
                    let inside = sheet == clip.sheet
                        && at.row >= block.0
                        && at.row <= block.2
                        && at.col >= block.1
                        && at.col <= block.3;
                    if inside {
                        continue;
                    }
                    let Some(mut out) = session
                        .workbook()
                        .sheets
                        .get(sheet)
                        .and_then(|s| s.cells.get(at))
                        .cloned()
                    else {
                        continue;
                    };
                    out.formula = Some(session.workbook_mut().store_formula(expr));
                    front.push(EditOperation::SetCell {
                        sheet,
                        at,
                        cell: Some(out),
                    });
                }
                // Defined names name cells too, and a name is the
                // indirection people use *so that* they need not track
                // addresses -- the worst place for one to go stale
                // (`UX-CUT-04`). `SetDefinedNames` inverts, so this joins the
                // same undo step.
                if let Some(names) = casual_calc_transaction::defined_names_after_move(
                    session.workbook(),
                    &sheet_name,
                    block,
                    (dr, dc),
                ) {
                    front.push(EditOperation::SetDefinedNames(names));
                }
                ops.splice(0..0, front);
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

pub(crate) fn apply_style_range(
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
