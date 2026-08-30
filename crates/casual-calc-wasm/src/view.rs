//! What the viewer sees rather than what the workbook holds: freeze,
//! gridlines, personal views, page setup and printing.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// Drop this participant's view of one sheet.
#[wasm_bindgen]
pub fn session_clear_personal_view(sheet: usize) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.clear_personal_view(sheet);
        Ok(())
    })
}

/// Drop every personal view.
///
/// A first-class command because undo will not do it: a personal view is not a
/// document edit, so undo after applying one reverses the last change to the
/// *document* instead.
#[wasm_bindgen]
pub fn session_clear_all_personal_views() -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        session.clear_all_personal_views();
        Ok(())
    })
}

/// Whether this participant has a personal view on `sheet`, for chrome that
/// offers to clear it.
#[wasm_bindgen]
pub fn session_has_personal_view(sheet: usize) -> bool {
    with_session(|s| s.views().has_view(sheet)).unwrap_or(false)
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

/// A sheet's display switches as JSON `{formulas, zeros}`.
///
/// Both were imported, exported and never shown: a file saved with "show
/// formulas" on opened displaying results, and saved back claiming otherwise.
///
/// `rightToLeft` is deliberately not here. It is modelled and round-trips, but
/// the canvas lays columns out left to right, so a switch for it would change
/// the file and nothing on screen.
#[wasm_bindgen]
pub fn session_view_options(sheet: usize) -> String {
    with_session(|s| {
        let v = &s.workbook().sheets.get(sheet)?.view;
        Some(format!(
            "{{\"formulas\":{},\"zeros\":{}}}",
            v.show_formulas, !v.hide_zeros
        ))
    })
    .flatten()
    .unwrap_or_else(|| "{\"formulas\":false,\"zeros\":true}".to_owned())
}

/// Set one of those switches (undoable). `which` is `formulas` or `zeros`; `on`
/// is what the user asked for, so `zeros` means *show* zeros and is stored
/// inverted, matching the OOXML attribute rather than the checkbox.
#[wasm_bindgen]
pub fn session_set_view_option(sheet: usize, which: &str, on: bool) -> Result<(), JsError> {
    let which = which.to_owned();
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(mut op) = current_sheet_metadata(session, sheet) else {
            return Ok(());
        };
        if let EditOperation::SetSheetMetadata { data, .. } = &mut op {
            match which.as_str() {
                "formulas" => data.view.show_formulas = on,
                "zeros" => data.view.hide_zeros = !on,
                _ => return Ok(()),
            }
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

/// Write page-setup attributes (one undo step).
///
/// `keys` are the same `group.attribute` names `session_page_setup` reports; an
/// empty value removes the attribute, because OOXML's defaults are meaningful
/// and writing `orientation=""` is not the same as leaving it out.
#[wasm_bindgen]
pub fn session_set_page_setup(
    sheet: usize,
    keys: Vec<String>,
    values: Vec<String>,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        for (key, value) in keys.iter().zip(values.iter()) {
            let Some((group, attr)) = key.split_once('.') else {
                continue;
            };
            let map = match group {
                "page" => &mut data.print.page,
                "margins" => &mut data.print.margins,
                "options" => &mut data.print.options,
                "setupPr" => &mut data.print.setup_pr,
                "hf" => &mut data.print.header_footer_text,
                _ => continue,
            };
            if value.is_empty() {
                map.remove(attr);
            } else {
                map.insert(attr.to_owned(), value.clone());
            }
        }
    })
}

/// Build a sheet-scoped defined name from an A1 reference, replacing any
/// existing one of that name on that sheet.
///
/// `Print_Area` and `Print_Titles` are ordinary defined names with reserved
/// names and a sheet scope — that is the whole mechanism, and there is no
/// separate element for either.
pub(crate) fn set_sheet_name(
    sheet: usize,
    name: &str,
    refers_to: Option<String>,
) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(id) = session.workbook().sheets.get(sheet).map(|sh| sh.id) else {
            return Ok(());
        };
        let mut names = session.workbook().defined_names.clone();
        names.retain(|d| !(d.sheet == Some(id) && d.name == name));
        if let Some(text) = refers_to {
            // `Print_Titles` is a whole-row reference (`Sheet1!$1:$2`), which
            // this parser does not read. Keeping the text verbatim writes
            // exactly what Excel expects, where refusing would mean the feature
            // could not exist until the parser grows.
            let expr = parse(&text).unwrap_or_else(|_| Expr::Raw(text.clone()));
            names.push(DefinedName {
                name: name.to_owned(),
                sheet: Some(id),
                formula: expr,
            });
        }
        session
            .edit(EditOperation::SetDefinedNames(names))
            .map_err(js)
    })
}

/// A sheet name as a formula prefix: quoted, with any inner quote doubled.
///
/// Unconditional quoting rather than "only when it needs it": the rules for
/// when a bare name is legal are fiddly, and a wrongly-unquoted name produces a
/// print area pointing at nothing.
pub(crate) fn sheet_prefix(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

/// Set the sheet's print area to a range.
#[wasm_bindgen]
pub fn session_set_print_area(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let Some(name) =
        with_session(|s| s.workbook().sheets.get(sheet).map(|sh| sh.name.clone())).flatten()
    else {
        return Ok(());
    };
    let a1 = |r: u32, c: u32| format!("${}${}", casual_calc_formula::column_to_letters(c), r + 1);
    let refers = format!(
        "{}!{}:{}",
        sheet_prefix(&name),
        a1(r0.min(r1), c0.min(c1)),
        a1(r0.max(r1), c0.max(c1))
    );
    set_sheet_name(sheet, "Print_Area", Some(refers))
}

/// Clear the sheet's print area, so the whole used region prints again.
#[wasm_bindgen]
pub fn session_clear_print_area(sheet: usize) -> Result<(), JsError> {
    set_sheet_name(sheet, "Print_Area", None)
}

/// Repeat rows `r0..=r1` at the top of every printed page — Excel's
/// "Print Titles". Pass `r1 < r0` to clear.
#[wasm_bindgen]
pub fn session_set_print_title_rows(sheet: usize, r0: u32, r1: u32) -> Result<(), JsError> {
    let Some(name) =
        with_session(|s| s.workbook().sheets.get(sheet).map(|sh| sh.name.clone())).flatten()
    else {
        return Ok(());
    };
    if r1 < r0 {
        return set_sheet_name(sheet, "Print_Titles", None);
    }
    // A row-only title is written as a whole-row reference, `$1:$2`.
    let refers = format!("{}!${}:${}", sheet_prefix(&name), r0 + 1, r1 + 1);
    set_sheet_name(sheet, "Print_Titles", Some(refers))
}

/// The sheet's print area and title rows as JSON, for the panel.
#[wasm_bindgen]
pub fn session_print_scope(sheet: usize) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let id = wb.sheets.get(sheet)?.id;
        let find = |n: &str| {
            wb.defined_names
                .iter()
                .find(|d| d.sheet == Some(id) && d.name == n)
                .map(|d| d.formula.to_string())
        };
        Some(format!(
            "{{\"area\":{},\"titles\":{}}}",
            json_string(&find("Print_Area").unwrap_or_default()),
            json_string(&find("Print_Titles").unwrap_or_default()),
        ))
    })
    .flatten()
    .unwrap_or_else(|| "{\"area\":\"\",\"titles\":\"\"}".to_owned())
}

/// A sheet's manual page breaks as JSON `{rows:[…],cols:[…]}` — zero-based
/// indices of the row/column each break sits *before*.
///
/// OOXML stores `<brk id>` one-based, matching the row number a user sees;
/// everything else here is zero-based, so the conversion happens at this
/// boundary rather than being repeated by each caller.
#[wasm_bindgen]
pub fn session_page_breaks(sheet: usize) -> String {
    with_session(|s| {
        let p = &s.workbook().sheets.get(sheet)?.print;
        let ids = |breaks: &[std::collections::BTreeMap<String, String>]| {
            let mut out: Vec<u32> = breaks
                .iter()
                .filter_map(|b| b.get("id")?.parse::<u32>().ok())
                .filter_map(|id| id.checked_sub(1))
                .collect();
            out.sort_unstable();
            out.dedup();
            out
        };
        let fmt = |v: Vec<u32>| v.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        Some(format!(
            "{{\"rows\":[{}],\"cols\":[{}]}}",
            fmt(ids(&p.row_breaks)),
            fmt(ids(&p.col_breaks))
        ))
    })
    .flatten()
    .unwrap_or_else(|| "{\"rows\":[],\"cols\":[]}".to_owned())
}

/// Add or remove a manual page break before `row` and/or before `col`.
///
/// Excel's "Insert Page Break" on a cell inserts both at once; on a whole-row
/// or whole-column selection it inserts only the one that makes sense. Passing
/// a `u32::MAX` for either axis skips it, which is how the host says "rows
/// only". A break already there is removed, so the command is a toggle.
#[wasm_bindgen]
pub fn session_toggle_page_break(sheet: usize, row: u32, col: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        let toggle = |breaks: &mut Vec<std::collections::BTreeMap<String, String>>, at: u32| {
            if at == u32::MAX || at == 0 {
                // A break before the first line is what the page edge already
                // is; Excel refuses it rather than writing one that does
                // nothing.
                return;
            }
            let id = (at + 1).to_string();
            if let Some(i) = breaks.iter().position(|b| b.get("id") == Some(&id)) {
                breaks.remove(i);
                return;
            }
            let mut brk: std::collections::BTreeMap<String, String> = Default::default();
            brk.insert("id".to_owned(), id);
            // `man="1"` is what distinguishes a break the user asked for from
            // one Excel computed; without it the break is discarded on the next
            // repagination.
            brk.insert("man".to_owned(), "1".to_owned());
            breaks.push(brk);
            breaks.sort_by_key(|b| b.get("id").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0));
        };
        toggle(&mut data.print.row_breaks, row);
        toggle(&mut data.print.col_breaks, col);
    })
}

/// The pictures on a sheet as JSON `[{r0,c0,r1,c1,part}]`.
///
/// The bytes are fetched separately by `session_image_data`, because a sheet
/// with a dozen photographs would otherwise send megabytes of base64 on every
/// frame the host asks where things are.
#[wasm_bindgen]
pub fn session_images(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .images
            .iter()
            .map(|im| {
                format!(
                    "{{\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{},\
                     \"fx\":{},\"fy\":{},\"tx\":{},\"ty\":{},\"part\":{}}}",
                    im.anchor.start.row,
                    im.anchor.start.col,
                    im.anchor.end.row,
                    im.anchor.end.col,
                    im.from_offset.x,
                    im.from_offset.y,
                    im.to_offset.x,
                    im.to_offset.y,
                    json_string(&im.part)
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// One piece of a resolved header or footer section.
///
/// A page number is not a string: nothing in the engine knows which page a
/// given line of the printout lands on, because nothing here paginates. It
/// stays a *token* all the way to the emitter, which turns it into a CSS page
/// counter — the one mechanism that can count pages in a document the browser
/// is breaking up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HfPart {
    /// Literal text, already substituted. Not yet escaped for any syntax.
    Text(String),
    /// `&P` — the number of the page this header is printed on.
    PageNumber,
    /// `&N` — how many pages the printout has in total.
    PageCount,
}

/// The values a header/footer field code can be substituted from.
///
/// The engine reads no clock, so `now` is whatever the host last passed to
/// [`crate::session_set_clock`]; `None` when it has passed none, in which case
/// `&D` and `&T` resolve to nothing rather than to an invented date.
pub(crate) struct HfContext<'a> {
    /// The sheet's tab name, for `&A`.
    pub sheet: &'a str,
    /// The workbook's file name, for `&F`, and its path for `&Z`. Empty when
    /// the host has not named the document.
    pub file: &'a str,
    /// The host's clock as a date serial, for `&D` and `&T`.
    pub now: Option<f64>,
}

/// Resolve an OOXML header/footer string into its left, centre and right
/// sections with every field code substituted.
///
/// The codes are `&L`/`&C`/`&R` (which section the text after them belongs
/// to), value codes like `&P` (page number), `&A` (sheet name) and `&D` (date),
/// and formatting codes like `&B` (bold) or `&"Arial,Bold"` (font). Text before
/// any section code is centred, which is what Excel does with an unmarked
/// header.
///
/// Formatting codes are consumed and dropped rather than applied. They are
/// per-run, and the sections are emitted into CSS `content:` strings, which
/// carry no markup — a run cannot change font in the middle of one. Dropping
/// them loses emphasis; printing them would put `&B` on the paper.
///
/// This replaces a `strip_hf_codes` that turned every code into a space, so
/// `&LSales&C&P` printed "Sales" and the page number the dialog advertises
/// could not appear at all.
pub(crate) fn hf_sections(raw: &str, ctx: &HfContext) -> [Vec<HfPart>; 3] {
    let mut sections: [Vec<HfPart>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    // Unmarked text is centred, as Excel treats it.
    let mut current = 1usize;
    let mut buf = String::new();

    // Held until the section is switched or the string ends, so consecutive
    // literal characters become one part rather than one part each.
    macro_rules! flush {
        ($sections:expr, $current:expr, $buf:expr) => {
            if !$buf.is_empty() {
                $sections[$current].push(HfPart::Text(std::mem::take(&mut $buf)));
            }
        };
    }

    let stamp = |code: &str| {
        ctx.now
            .map(|serial| casual_calc_layout::format_number(serial, code))
            .unwrap_or_default()
    };

    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            buf.push(ch);
            continue;
        }
        let Some(code) = chars.next() else {
            // A trailing bare `&` is not a code. Excel writes `&&` for a
            // literal one, but a file that ends mid-code should still print
            // its text rather than swallow the character.
            buf.push('&');
            break;
        };
        match code {
            // A quoted font name: skip to the closing quote.
            '"' => {
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
            }
            // `&&` is a literal ampersand.
            '&' => buf.push('&'),
            // A point size is `&` followed by digits.
            d if d.is_ascii_digit() => {
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
            }
            'L' | 'C' | 'R' => {
                flush!(sections, current, buf);
                current = match code {
                    'L' => 0,
                    'C' => 1,
                    _ => 2,
                };
            }
            'P' => {
                flush!(sections, current, buf);
                sections[current].push(HfPart::PageNumber);
            }
            'N' => {
                flush!(sections, current, buf);
                sections[current].push(HfPart::PageCount);
            }
            'A' => buf.push_str(ctx.sheet),
            'F' | 'Z' => buf.push_str(ctx.file),
            'D' => buf.push_str(&stamp("yyyy-mm-dd")),
            'T' => buf.push_str(&stamp("hh:mm:ss")),
            // `&K` carries a six-digit colour, or a two-digit theme index and
            // a tint; the six-digit form is the one Excel writes. Its digits
            // are not text and must not print.
            'K' => {
                for _ in 0..6 {
                    if chars.peek().is_some_and(char::is_ascii_hexdigit) {
                        chars.next();
                    }
                }
            }
            // Every remaining code is a formatting toggle (`&B`, `&I`, `&U`,
            // `&E`, `&S`, `&X`, `&Y`, `&O`, `&H`), a picture (`&G`), or a
            // reserved letter. All are consumed and none print.
            _ => {}
        }
    }
    flush!(sections, current, buf);

    for section in &mut sections {
        trim_edges(section);
    }
    sections
}

/// Drop leading and trailing whitespace from a resolved section, so a header
/// written `&L Sales &C` does not print with the spaces that only separated
/// its codes.
fn trim_edges(parts: &mut Vec<HfPart>) {
    if let Some(HfPart::Text(first)) = parts.first_mut() {
        let trimmed = first.trim_start().to_owned();
        *first = trimmed;
    }
    if let Some(HfPart::Text(last)) = parts.last_mut() {
        let trimmed = last.trim_end().to_owned();
        *last = trimmed;
    }
    parts.retain(|p| !matches!(p, HfPart::Text(t) if t.is_empty()));
}

/// The decision behind [`guard_protected`], separated from the refusal.
///
/// A `JsError` cannot be constructed off-wasm, so a test that exercised the
/// guard could only ever panic. The rule is the part worth testing.
pub(crate) fn protection_blocks(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> bool {
    with_session(|s| {
        let wb = s.workbook();
        let Some(sh) = wb.sheets.get(sheet) else {
            return false;
        };
        if !sh.protection.as_ref().is_some_and(|p| p.is_enabled()) {
            return false;
        }
        let locked = |row: u32, col: u32| {
            sh.cells
                .get(CellRef::new(row, col))
                .and_then(|c| c.style)
                .and_then(|id| wb.styles.get(id))
                .and_then(|st| st.locked)
                .unwrap_or(true)
        };
        // An empty cell carries no style and is therefore locked, so scanning
        // the corners is not enough — but scanning a huge range cell by cell is
        // not either. Any locked cell in the block refuses the whole edit,
        // which is what Excel does, so the first one found is the answer.
        (r0..=r1).any(|r| (c0..=c1).any(|c| locked(r, c)))
    })
    .unwrap_or(false)
}

/// Whether a cell's formula is hidden by protection — `<protection hidden="1">`
/// on a protected sheet. The formula bar shows the value instead.
pub(crate) fn formula_is_hidden(wb: &Workbook, sheet: &Sheet, at: CellRef) -> bool {
    sheet.protection.as_ref().is_some_and(|p| p.is_enabled())
        && sheet
            .cells
            .get(at)
            .and_then(|c| c.style)
            .and_then(|id| wb.styles.get(id))
            .and_then(|st| st.formula_hidden)
            .unwrap_or(false)
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

#[cfg(test)]
mod field_code_agreement {
    use super::*;

    /// The HTML print path and the PDF must read a header the same way
    /// (`IO-11`).
    ///
    /// There were two parsers for one language. `hf_sections` here, and
    /// `casual_calc_layout::print::parse_header_footer`, which `IO-10` wrote
    /// for the PDF. The layout one is the owner — it is the one with the
    /// refusals, the `differentFirst`/`differentOddEven` variants and the
    /// per-run styling — and this one silently dropped `&B` and `&I` and knew
    /// nothing about the variants.
    ///
    /// The cost was one file printing two different headers: the browser's and
    /// the PDF's, from the same bytes. `print.rs`'s own module doc names that
    /// as the failure mode of a second implementation, which is what makes this
    /// worth a test rather than a comment.
    ///
    /// Compares the **text a reader sees**, not the internal shape: the two
    /// representations differ on purpose — CSS `content:` carries no markup, so
    /// this side cannot express a bold run — and demanding they match
    /// structurally would force one of them to become the other.
    fn visible_text(parts: &[HfPart]) -> String {
        parts
            .iter()
            .map(|p| match p {
                HfPart::Text(t) => t.clone(),
                HfPart::PageNumber => "\u{1}P".to_owned(),
                HfPart::PageCount => "\u{1}N".to_owned(),
            })
            .collect()
    }

    fn layout_text(raw: &str, sheet: &str) -> [String; 3] {
        use casual_calc_layout::print::{HeaderField, PrintContext, parse_header_footer};
        let mut refused = std::collections::BTreeMap::new();
        let hf = parse_header_footer(raw, sheet, &PrintContext::default(), &mut refused);
        std::array::from_fn(|i| {
            hf.sections[i]
                .iter()
                .map(|piece| match &piece.field {
                    HeaderField::Text(t) => t.clone(),
                    HeaderField::PageNumber(_) => "\u{1}P".to_owned(),
                    HeaderField::PageCount => "\u{1}N".to_owned(),
                    HeaderField::LineBreak => "\n".to_owned(),
                })
                .collect()
        })
    }

    #[test]
    fn both_paths_read_the_same_header_the_same_way() {
        // `&B` and `&I` are the codes this side used to drop on the floor;
        // a plain run either side of them is what shows the drop is silent.
        let raw = "&LSales&C&BQ3 Summary&B page &P of &N&R&I&A";
        let ctx = HfContext {
            file: "",
            now: None,
            sheet: "Sheet1",
        };
        let here = hf_sections(raw, &ctx);
        let there = layout_text(raw, "Sheet1");

        for (i, name) in ["left", "centre", "right"].iter().enumerate() {
            assert_eq!(
                visible_text(&here[i]),
                there[i],
                "the {name} section reads differently in the browser and in the PDF, \
                 from the same bytes — which is one document with two headers"
            );
        }
    }
}
