//! Everything that changes how a cell looks without changing what it holds.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

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

/// Column widths in device pixels (96 dpi) for `count` columns starting at
/// `first`, as a JSON array. Lets the editor draw real `.xlsx` column widths.
#[wasm_bindgen]
pub fn session_col_px(sheet: usize, first: u32, count: u32) -> String {
    axis_px(sheet, first, count, DEFAULT_COL_WIDTH, true)
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
pub(crate) fn range_all(
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
pub(crate) const FILL_LISTS: &[&[&str]] = &[
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
pub(crate) fn find_in_fill_lists(text: &str) -> Option<(usize, usize)> {
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
pub(crate) fn detect_text_series(vals: &[Option<String>]) -> Option<(usize, i64, i64)> {
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
pub(crate) fn text_series_at(list_id: usize, idx0: i64, step: i64, k: i64) -> String {
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
/// The theme link for a picker's `(slot, tint)`, or `None` when the slot is
/// negative — the editor's way of saying "this colour is not from the theme".
pub(crate) fn theme_link(slot: i32, tint: f64) -> Option<ThemeTint> {
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
            // The four edges, each as its OOXML line-style token. Emitted per
            // edge because that is how the model stores them and how a paste
            // sets them: `session_range_bordered` answers only "are all four
            // present across this whole range", which cannot see a cell that
            // carries a bottom rule and nothing else.
            if let Some(border) = &st.border {
                let edges: Vec<String> = [
                    ("t", &border.top),
                    ("r", &border.right),
                    ("b", &border.bottom),
                    ("l", &border.left),
                ]
                .iter()
                .filter_map(|(name, edge)| {
                    edge.as_ref()
                        .map(|e| format!("{}:{}", json_string(name), json_string(&e.style)))
                })
                .collect();
                if !edges.is_empty() {
                    parts.push(format!("\"bd\":{{{}}}", edges.join(",")));
                }
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
pub(crate) fn border_edges(
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
pub(crate) fn full_thin_border() -> Borders {
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
    guard_protected(sheet, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))?;
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
pub(crate) fn apply_style_range_pos(
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

/// How many fonts have been supplied at runtime.
#[wasm_bindgen]
#[must_use]
pub fn registered_font_count() -> usize {
    casual_calc_render::registered_count()
}

/// Which scripts in `text` no available face can draw, as JSON:
/// `[{"script":"Thai","sample":"ไ"}]`, empty for the ordinary document.
///
/// The counterpart to [`register_font`]. Registering is how a host fixes the
/// gap; this is how it finds out there is one, before a user sees a picture of
/// boxes and files a rendering bug.
///
/// Answers for whatever is registered at the moment of the call, so ask after
/// registering. Only the headless PNG path needs it — the editor draws through
/// the browser, which brings its own faces.
#[wasm_bindgen]
#[must_use]
pub fn missing_font_scripts(text: &str) -> String {
    let missing: Vec<serde_json::Value> = casual_calc_render::missing_scripts(text)
        .into_iter()
        .map(|m| serde_json::json!({ "script": m.script, "sample": m.sample.to_string() }))
        .collect();
    serde_json::to_string(&missing).unwrap_or_else(|_| "[]".to_owned())
}

/// The protocol version this engine speaks.
///
/// Checked by the server on the first message, so a mismatched pair stops at
/// once rather than proceeding until a missing field produces something more
/// confusing. Exposed because the *client* has to state it, and hard-coding the
/// number in JavaScript is how the two drift.
#[wasm_bindgen]
pub fn protocol_version() -> u32 {
    casual_calc_transaction::protocol::PROTOCOL_VERSION
}
