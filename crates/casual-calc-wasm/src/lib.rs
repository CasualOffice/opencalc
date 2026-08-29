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
use std::collections::BTreeSet;

use casual_calc_eval::{Recalculated, recalculate};
use casual_calc_formula::stored::{ABSOLUTE, Origin, StoredRef};
use casual_calc_formula::{Expr, parse, restore_at};
use casual_calc_layout::table_style::table_style_colors;
use casual_calc_layout::{
    DEFAULT_COL_WIDTH, DEFAULT_ROW_HEIGHT, GridGeometry, Viewport, display_color, display_text,
};
use casual_calc_model::{
    AutoFilter, BorderEdge, Borders, Cell, CellComment, CellRange, CellRef, CellValue, CfRule,
    ChartKind, ChartView, CommentReply, ConditionalFormat, CustomFilter, DataValidation,
    DefinedName, FilterOp, FilterRule, HAlign, Hyperlink, Id, PivotAggregate, PivotAxisField,
    PivotFilterField, PivotSort, PivotTable, PivotValueField, Sheet, SheetId, SheetVisibility,
    Style, StyleId, Table, ThemeTint, Underline, VAlign, VertAlign, Workbook,
};
use casual_calc_sdk::{EditOperation, SheetMetadata, WorkbookSession, render_sheet_png};
use casual_calc_transaction::protocol::ClientMessage;
use casual_calc_transaction::session::ClientSession;
use wasm_bindgen::prelude::*;

// The bridge's surfaces. Unlike the formula library (`MNT-002`) this file
// carried almost no section headings, so the seams were read off how the
// exports actually cluster — and several topics turned out to appear in two
// or three places, which is what made the file unreviewable.
//
// Re-exported flat, and `pub` rather than `pub(crate)`: every one of these was
// a `pub fn` at the crate root before, so anything narrower would move items
// out of the public API as a side effect of tidying the file. The modules stay
// private, so the root is still the only path to them — exactly as it was.
mod axis;
mod calc;
mod cells;
mod clipboard;
mod collab;
mod data;
mod formula;
mod history;
mod io;
mod objects;
mod sheet;
mod structural;
mod style;
mod view;

pub use axis::*;
pub use calc::*;
pub use cells::*;
pub use clipboard::*;
pub use collab::*;
pub use data::*;
pub use formula::*;
pub use history::*;
pub use io::*;
pub use objects::*;
pub use sheet::*;
pub use structural::*;
pub use style::*;
pub use view::*;

thread_local! {
    static SESSION: RefCell<Option<WorkbookSession>> = const { RefCell::new(None) };
    /// How long the next long job may hold the thread, in milliseconds, or
    /// `None` for "until it finishes". See [`session_set_time_budget_ms`].
    static TIME_BUDGET_MS: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

// ---------------------------------------------------------------------------
// Stopping a long job (`SEC-017`).
//
// `SEC-012` made admission and full recalculation cancellable and no host took
// the seam, so the scenario `casual_calc_model::cancel`'s own header describes
// — a workbook inside every limit and simply enormous holding the one thread a
// browser has — was still exactly what a browser user got. This is the host
// side of that seam.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
extern "C" {
    /// The browser's monotonic clock, in milliseconds.
    ///
    /// Imported rather than taken from `std`, because
    /// `std::time::Instant::now` **panics** on `wasm32-unknown-unknown` — which
    /// is the reason [`casual_calc_model::Cancel`] is implemented for any
    /// `Fn() -> bool` and leaves the clock to the host. This bridge is that
    /// host, and `performance.now()` is the clock this target has.
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

/// Give the next long job a wall-clock budget, in milliseconds.
///
/// **A deadline, not a flag, because a flag cannot work here.** JavaScript and
/// WebAssembly share the one thread a tab has, so nothing outside a running job
/// can raise anything while it runs: a `stop()` call would not execute until
/// the job it was meant to stop had already returned. The only token that can
/// fire *during* a long call is one the job evaluates itself, and that is a
/// clock.
///
/// The budget stays in force until it is changed or cleared, and it bounds each
/// job separately — the deadline is taken when the job starts, not when the
/// budget was set. It applies to [`session_open`] / [`session_open_as`] (a
/// cancelled open loads **nothing** and leaves any previous session in place)
/// and to [`session_recalculate`] (a cancelled recalculation keeps what it
/// computed and reports that it did).
///
/// A negative or non-finite budget means the same as
/// [`session_clear_time_budget`].
#[wasm_bindgen]
pub fn session_set_time_budget_ms(ms: f64) {
    let budget = (ms.is_finite() && ms >= 0.0).then_some(ms);
    TIME_BUDGET_MS.with(|b| b.set(budget));
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

pub(crate) fn set_session(session: WorkbookSession) {
    SESSION.with(|cell| *cell.borrow_mut() = Some(session));
}

pub(crate) fn with_session<R>(f: impl FnOnce(&WorkbookSession) -> R) -> Option<R> {
    SESSION.with(|cell| cell.borrow().as_ref().map(f))
}

/// A cell's editable content: `=formula` for a formula cell, otherwise the
/// value as it would be typed. Find & Replace operate on this (Excel's default
/// "Formulas" look-in) so a match is always something Replace can rewrite.
pub(crate) fn cell_input_text(wb: &Workbook, cell: &Cell) -> String {
    if let Some(handle) = cell.formula
        && let Some(expr) = wb.formula(handle)
    {
        return format!("={expr}");
    }
    value_text(wb, &cell.value)
}

pub(crate) fn value_text(workbook: &Workbook, value: &CellValue) -> String {
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

pub(crate) fn viewport_px(width_px: u32, height_px: u32, dpi: u32) -> Viewport {
    Viewport {
        x: 0,
        y: 0,
        width: px_to_twips(width_px, dpi),
        height: px_to_twips(height_px, dpi),
    }
}

pub(crate) fn px_to_twips(px: u32, dpi: u32) -> i64 {
    if dpi == 0 {
        return 0;
    }
    px as i64 * 1440 / dpi as i64
}

pub(crate) fn js<E: std::fmt::Display>(err: E) -> JsError {
    JsError::new(&err.to_string())
}

/// A cell's borders as JSON `{ "l": "style:color", ... }` — one key per present
/// edge (l/r/t/b), value `"<line-style>:<RRGGBB or empty>"`.
pub(crate) fn border_json(b: &Borders) -> String {
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

pub(crate) fn json_string(s: &str) -> String {
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
    use super::{ci_replace, html_cell_css, push_html_escaped};
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

    /// **An eight-digit OOXML colour is `AARRGGBB`; CSS reads `RRGGBBAA`.**
    ///
    /// Emitted unchanged, the alpha becomes the red channel: an opaque black
    /// `FF000000` reads as `#FF0000` at zero alpha — fully transparent red. The
    /// cell loses its colour on paste and in the printout, and nothing reports
    /// it, because the string is a valid colour in both notations and only
    /// means different things.
    #[test]
    fn an_eight_digit_colour_is_reordered_from_ooxml_to_css() {
        let style = Style {
            font_color: Some("FF000000".to_owned()), // opaque black
            fill_color: Some("80FF0000".to_owned()), // half-transparent red
            ..Style::default()
        };
        let css = html_cell_css(&style);
        assert!(
            css.contains("color:#000000FF;"),
            "opaque black must stay black, not become transparent red: {css}"
        );
        assert!(
            css.contains("background-color:#FF000080;"),
            "the alpha belongs last in CSS: {css}"
        );
    }

    /// **A colour is hex, or it is not emitted.**
    ///
    /// `style.font_color` is whatever the file's `styles.xml` said — preserved
    /// verbatim on import, as docs/34 requires — and this string is dropped
    /// into a `style="…"` attribute that `session_print_html` hands to
    /// `document.write` in a window inheriting the editor's origin. A workbook
    /// that closed the attribute and opened an `<img onerror=…>` ran script
    /// next to the session token and the collaboration socket, with a live
    /// `window.opener`.
    ///
    /// The CI SEC-001 sink check could not see this: it greps `webapp/*.js` and
    /// the host's HTML, and this markup is assembled in Rust.
    #[test]
    fn a_workbook_colour_cannot_escape_the_style_attribute() {
        let hostile = Style {
            font_color: Some("a\"><img src=x onerror=alert(1)>".to_owned()),
            fill_color: Some("</style><script>alert(1)</script>".to_owned()),
            ..Style::default()
        };
        let css = html_cell_css(&hostile);
        assert!(
            !css.contains('<') && !css.contains('"') && !css.contains('>'),
            "workbook text reached a style attribute: {css:?}"
        );
        assert_eq!(css, "", "a colour that is not a colour is simply dropped");

        // The shapes a real file uses still come through. Three and six digits
        // are the same in both notations and pass through unchanged; **eight
        // are not**, and this loop used to assert they did — an OOXML
        // `AARRGGBB` emitted verbatim is read by CSS as `RRGGBBAA`, so an
        // opaque red `FFFF0000` became `#FFFF00` at zero alpha, a transparent
        // yellow. See `an_eight_digit_colour_is_reordered_from_ooxml_to_css`
        // and `FID-35`; the assertion here was encoding the defect.
        for (good, css) in [
            ("FF0000", "FF0000"),
            ("F00", "F00"),
            ("FFFF0000", "FF0000FF"),
        ] {
            let style = Style {
                font_color: Some(good.to_owned()),
                ..Style::default()
            };
            assert_eq!(html_cell_css(&style), format!("color:#{css};"));
        }
    }

    /// **Case-insensitive replace must survive text that changes length when
    /// lowercased.**
    ///
    /// The old implementation lowercased the haystack once and then indexed it
    /// with byte offsets taken from the original, which only works if
    /// lowercasing preserves length. `İ` grows to two characters and the Kelvin
    /// sign `K` shrinks to one byte, so the two drifted apart and the slice
    /// landed out of bounds or inside a character.
    ///
    /// A panic here is not a caught error: on wasm32 it traps, aborts the
    /// module, leaves the `RefCell` borrow held across it locked forever, and
    /// takes the open workbook with it. An ordinary Turkish column heading was
    /// enough, through the default Find bar path — "match case" is off by
    /// default.
    #[test]
    fn case_insensitive_replace_handles_text_that_changes_length_when_lowercased() {
        // U+0130, which lowercases to two characters.
        assert_eq!(ci_replace("Ürün İsmi", "smi", "X"), "Ürün İX");
        // U+212A KELVIN SIGN, which lowercases to a shorter byte sequence.
        assert_eq!(
            ci_replace("300\u{212A} sample", "sample", "X"),
            "300\u{212A} X"
        );
        // The ordinary cases, unchanged.
        assert_eq!(ci_replace("Hello World", "world", "there"), "Hello there");
        assert_eq!(ci_replace("aAaA", "a", "-"), "----");
        assert_eq!(ci_replace("nothing", "", "X"), "nothing");
        assert_eq!(ci_replace("no match here", "zzz", "X"), "no match here");
        // Matching across a case fold that changes length.
        assert_eq!(ci_replace("İstanbul", "i\u{307}stanbul", "X"), "X");
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

    /// A cut **moves** a cell, so its formula travels verbatim; a copy shifts.
    ///
    /// The paste path shifted references by the per-cell delta whichever it
    /// was, so cutting `=A1+1` from B1 to D5 rewrote it to `=C5+1` — a formula
    /// pointing somewhere it had never referred to, produced by an everyday
    /// action, with nothing to show it had happened.
    #[test]
    fn a_cut_moves_a_formula_verbatim_where_a_copy_shifts_it() {
        use super::{
            session_cell_input, session_clip_copy, session_clip_paste_mode, session_new,
            session_set_cell,
        };

        // Copy first, to pin the behaviour that must *not* change.
        session_new();
        session_set_cell(0, 0, 0, "5").unwrap(); // A1
        session_set_cell(0, 1, 1, "=A1+1").unwrap(); // B2
        session_clip_copy(0, 1, 1, 1, 1, false); // copy B2
        session_clip_paste_mode(0, 4, 3, "all").unwrap(); // to D5 (dr=+3, dc=+2)
        assert_eq!(
            session_cell_input(0, 4, 3),
            "=C4+1",
            "a copy shifts by the delta"
        );

        // The same move as a cut: the formula is unchanged, and the source is
        // emptied because the cell went there rather than being duplicated.
        session_new();
        session_set_cell(0, 0, 0, "5").unwrap(); // A1
        session_set_cell(0, 1, 1, "=A1+1").unwrap(); // B2
        session_clip_copy(0, 1, 1, 1, 1, true); // cut B2
        session_clip_paste_mode(0, 4, 3, "all").unwrap(); // to D5
        assert_eq!(
            session_cell_input(0, 4, 3),
            "=A1+1",
            "a cut moves the cell, so the formula still means what it meant"
        );
        assert_eq!(session_cell_input(0, 1, 1), "", "and the source is emptied");
    }

    /// **Cutting a cell repoints every formula that named it.**
    ///
    /// The moved cell's own formula travels verbatim, which the test above
    /// pins. Nothing did the other half: `=A1*2` sitting in C1 kept saying
    /// `A1` after A1 was cut to E5, so it stopped reading the value it was
    /// written to read and silently began reading whatever moved in underneath
    /// -- usually nothing, so a live number became zero with no error and no
    /// visible cause (`UX-CUT-03`).
    #[test]
    fn a_cut_repoints_the_formulas_that_referred_to_it() {
        use super::{
            session_cell_input, session_clip_copy, session_clip_paste_mode, session_new,
            session_set_cell, session_undo,
        };

        session_new();
        session_set_cell(0, 0, 0, "5").unwrap(); // A1
        session_set_cell(0, 0, 2, "=A1*2").unwrap(); // C1 -> A1
        session_set_cell(0, 0, 3, "=$A$1+1").unwrap(); // D1, anchored
        session_set_cell(0, 0, 4, "=B1+1").unwrap(); // E1, a control: not moved
        session_set_cell(0, 1, 0, "=SUM(A1:A9)").unwrap(); // A2, partial overlap

        session_clip_copy(0, 0, 0, 0, 0, true); // cut A1
        session_clip_paste_mode(0, 5, 6, "all").unwrap(); // to G6

        assert_eq!(
            session_cell_input(0, 0, 2),
            "=G6*2",
            "the reference did not follow the cell it names"
        );
        // `$` is about what a *copy* does to a reference, not about whether the
        // cell it names may move. Excel moves both.
        assert_eq!(
            session_cell_input(0, 0, 3),
            "=$G$6+1",
            "an anchored reference names A1 too, and A1 has gone"
        );
        assert_eq!(
            session_cell_input(0, 0, 4),
            "=B1+1",
            "a formula naming a cell outside the block must be left alone"
        );
        assert_eq!(
            session_cell_input(0, 1, 0),
            "=SUM(A1:A9)",
            "a range only partly inside the block has no correct rewrite, so it keeps its shape"
        );

        // **One undo step.** The repoint rides in the cut's own batch, so a
        // user who undoes a move gets the whole move back -- not a half-undone
        // state with the data returned and the references still repointed.
        session_undo().unwrap();
        assert_eq!(session_cell_input(0, 0, 0), "5", "the cut is undone");
        assert_eq!(
            session_cell_input(0, 0, 2),
            "=A1*2",
            "the repoint was a separate undo step"
        );
    }

    /// **A cross-sheet reference to a moved cell follows it too.**
    ///
    /// Qualified or not, the question is the same one the insert/delete
    /// rewrite asks: does this reference reach the sheet the cells left? An
    /// unqualified `A1` on another sheet means *that* sheet's A1 and must not
    /// move -- getting this backwards would silently rewrite formulas on every
    /// sheet in the workbook.
    #[test]
    fn a_cut_repoints_across_sheets_without_touching_other_sheets_own_cells() {
        use super::{
            session_add_sheet, session_cell_input, session_clip_copy, session_clip_paste_mode,
            session_new, session_set_cell,
        };

        session_new();
        let second = session_add_sheet().unwrap();
        session_set_cell(0, 0, 0, "5").unwrap(); // Sheet1!A1, the cell to move
        // On the second sheet: one formula naming Sheet1 explicitly, and one
        // unqualified -- which means *this* sheet's A1, a different cell.
        session_set_cell(second, 0, 1, "=Sheet1!A1*2").unwrap();
        session_set_cell(second, 0, 2, "=A1*3").unwrap();

        session_clip_copy(0, 0, 0, 0, 0, true); // cut Sheet1!A1
        session_clip_paste_mode(0, 5, 6, "all").unwrap(); // to G6

        assert_eq!(
            session_cell_input(second, 0, 1),
            "=Sheet1!G6*2",
            "a qualified reference did not follow the cell across sheets"
        );
        assert_eq!(
            session_cell_input(second, 0, 2),
            "=A1*3",
            "an unqualified reference means this sheet's A1, which never moved"
        );
    }

    /// **A cut repoints the defined names that pointed at it.**
    ///
    /// `FID-24` made an insert or a delete shift defined names. A cut left
    /// them behind: `Rate` went on meaning `$A$1` after `$A$1` had gone to
    /// `G6`, so every formula written as `=Rate` silently read a different
    /// cell. A name is the indirection people reach for *so that* they need
    /// not track addresses, which makes it the worst place for one to go
    /// stale (`UX-CUT-04`).
    #[test]
    fn a_cut_repoints_the_defined_names_that_pointed_at_it() {
        use super::{
            session_cell_input, session_clip_copy, session_clip_paste_mode, session_define_name,
            session_names, session_new, session_set_cell, session_undo,
        };

        session_new();
        session_set_cell(0, 0, 0, "5").unwrap(); // A1, the cell to move
        session_set_cell(0, 0, 1, "9").unwrap(); // B1, never moved
        session_define_name("Rate", "Sheet1!$A$1").unwrap();
        session_define_name("Other", "Sheet1!$B$1").unwrap();
        session_set_cell(0, 2, 0, "=Rate*2").unwrap(); // A3, uses the name

        session_clip_copy(0, 0, 0, 0, 0, true); // cut A1
        session_clip_paste_mode(0, 5, 6, "all").unwrap(); // to G6

        let names = session_names();
        assert!(
            names.contains("Sheet1!$G$6"),
            "the name did not follow the cell it points at: {names}"
        );
        assert!(
            names.contains("Sheet1!$B$1"),
            "a name pointing outside the block must be left alone: {names}"
        );
        // The name still resolves to the value, which is the whole point of
        // repointing it rather than leaving a stale address that reads blank.
        assert_eq!(session_cell_input(0, 2, 0), "=Rate*2");

        // One undo step, as the move is one action.
        session_undo().unwrap();
        let restored = session_names();
        assert!(
            restored.contains("Sheet1!$A$1"),
            "undoing the cut left the name repointed: {restored}"
        );
    }

    /// Esc after a cut has to reach the engine.
    ///
    /// Clearing the marquee alone left the pending cut armed, so the next paste
    /// still moved the data and emptied the source the user believed they had
    /// spared. The visible signal said cancelled and the state said otherwise.
    #[test]
    fn clearing_the_clipboard_cancels_a_pending_cut() {
        use super::{
            session_cell_input, session_clip_clear, session_clip_copy, session_clip_has,
            session_clip_paste_mode, session_new, session_set_cell,
        };

        session_new();
        session_set_cell(0, 0, 0, "keep me").unwrap(); // A1
        session_clip_copy(0, 0, 0, 0, 0, true); // cut A1
        assert!(session_clip_has(), "the cut is armed");

        session_clip_clear();
        assert!(!session_clip_has(), "and Esc disarms it");

        // A paste now does nothing, and — the point — the source survives.
        let _ = session_clip_paste_mode(0, 4, 0, "all");
        assert_eq!(
            session_cell_input(0, 0, 0),
            "keep me",
            "the cancelled cut must not still move the data"
        );
        assert_eq!(session_cell_input(0, 4, 0), "", "and nothing was pasted");
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
        assert_eq!(session_cell_input(0, 4, 1), "=B4*10"); // B5

        // --- Formulas-only: value+formula in, target's formatting kept. ---
        session_new();
        session_set_cell(0, 0, 0, "5").unwrap(); // A1
        session_set_cell(0, 1, 0, "=A1+1").unwrap(); // A2 formula
        session_set_cell(0, 4, 3, "9").unwrap(); // D5 target
        session_toggle_bold(0, 4, 3, 4, 3).unwrap(); // bold D5
        session_clip_copy(0, 1, 0, 1, 0, false); // copy A2
        session_clip_paste_mode(0, 4, 3, "formulas").unwrap(); // onto D5
        // A2 moved to D5 (dr=+3, dc=+3): =A1+1 → =(D4+1).
        assert_eq!(session_cell_input(0, 4, 3), "=D4+1");
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
        SuffixSeries, detect_suffix_series, detect_text_series, is_day_format, session_cell_input,
        session_fill, session_new, session_set_cell, suffix_series_at, text_series_at,
    };

    fn series(prefix: &str, width: usize, start: i64, step: i64) -> Option<SuffixSeries> {
        Some(SuffixSeries {
            prefix: prefix.to_owned(),
            width,
            start,
            step,
        })
    }

    #[test]
    fn suffix_series_detection_and_its_edges() {
        // One cell counts by one; the padding width comes from the source.
        assert_eq!(
            detect_suffix_series(&[Some("Item 1".into())]),
            series("Item ", 1, 1, 1)
        );
        assert_eq!(
            detect_suffix_series(&[Some("Q01".into())]),
            series("Q", 2, 1, 1)
        );
        // Two cells set the step; identical text is required.
        assert_eq!(
            detect_suffix_series(&[Some("Q1".into()), Some("Q3".into())]),
            series("Q", 1, 1, 2)
        );
        assert_eq!(
            detect_suffix_series(&[Some("Item 1".into()), Some("Thing 2".into())]),
            None
        );
        // Uneven widths mean the source was not padding, so nor do we.
        assert_eq!(
            detect_suffix_series(&[Some("Item 9".into()), Some("Item 10".into())]),
            series("Item ", 1, 9, 1)
        );
        // No trailing digits, a bare numeral, a blank, and digits too long to
        // be an integer all tile rather than count.
        assert_eq!(detect_suffix_series(&[Some("Widget".into())]), None);
        assert_eq!(detect_suffix_series(&[Some("42".into())]), None);
        assert_eq!(detect_suffix_series(&[None]), None);
        assert_eq!(detect_suffix_series(&[]), None);
        assert_eq!(
            detect_suffix_series(&[Some("X99999999999999999999".into())]),
            None
        );

        // Rendering: padding kept, width grows, counting down goes negative
        // rather than wrapping, and an overflow refuses instead of panicking.
        let q = detect_suffix_series(&[Some("Q01".into())]).unwrap();
        assert_eq!(suffix_series_at(&q, 1).as_deref(), Some("Q02"));
        assert_eq!(suffix_series_at(&q, 98).as_deref(), Some("Q99"));
        assert_eq!(suffix_series_at(&q, 99).as_deref(), Some("Q100"));
        assert_eq!(suffix_series_at(&q, -2).as_deref(), Some("Q-1"));
        let huge = SuffixSeries {
            prefix: "N".into(),
            width: 1,
            start: i64::MAX,
            step: i64::MAX,
        };
        assert_eq!(suffix_series_at(&huge, 2), None);
        assert_eq!(suffix_series_at(&huge, 1), None);
    }

    #[test]
    fn only_calendar_formats_count_as_a_day() {
        assert!(is_day_format("yyyy-mm-dd"));
        assert!(is_day_format("d/m/yy"));
        assert!(is_day_format("[$-409]dddd"));
        // Time-only codes are dates to the formatter but have no day step.
        assert!(!is_day_format("hh:mm"));
        assert!(!is_day_format("[h]:mm:ss"));
        assert!(!is_day_format("0.00"));
        assert!(!is_day_format("General"));
        // A `d` inside quoted text or escaped is literal text, not a day.
        assert!(!is_day_format("0\" days\""));
        assert!(!is_day_format("0\\d"));
    }

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

    /// Dragging one date cell is the commonest fill there is, and it tiled:
    /// every cell came out `2024-01-01`. Excel and Sheets both walk the days.
    #[test]
    fn a_lone_date_fills_as_consecutive_days() {
        session_new();
        session_set_cell(0, 0, 0, "2024-01-01").unwrap(); // A1
        session_fill(0, 0, 0, 0, 0, 0, 0, 3, 0).unwrap(); // A1 down to A4
        assert_eq!(session_cell_input(0, 1, 0), "2024-01-02");
        assert_eq!(session_cell_input(0, 2, 0), "2024-01-03");
        assert_eq!(session_cell_input(0, 3, 0), "2024-01-04");

        // A leap day is a serial like any other — the calendar comes from the
        // formatter, not from the fill.
        session_new();
        session_set_cell(0, 0, 0, "2024-02-28").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 2, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "2024-02-29");
        assert_eq!(session_cell_input(0, 2, 0), "2024-03-01");

        // Filling upward walks backwards.
        session_new();
        session_set_cell(0, 4, 0, "2024-03-05").unwrap(); // A5
        session_fill(0, 4, 0, 4, 0, 2, 0, 4, 0).unwrap(); // A5 up to A3
        assert_eq!(session_cell_input(0, 3, 0), "2024-03-04");
        assert_eq!(session_cell_input(0, 2, 0), "2024-03-03");

        // Sideways too.
        session_new();
        session_set_cell(0, 0, 0, "2024-01-31").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 0, 2).unwrap(); // A1 right to C1
        assert_eq!(session_cell_input(0, 0, 1), "2024-02-01");
        assert_eq!(session_cell_input(0, 0, 2), "2024-02-02");

        // Ctrl-drag (explicit copy) still tiles the date.
        session_new();
        session_set_cell(0, 0, 0, "2024-01-01").unwrap();
        super::session_fill_mode(0, 0, 0, 0, 0, 0, 0, 2, 0, "copy").unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "2024-01-01");

        // A lone *time* has no day to step, so it copies — the step is the
        // calendar's, not every date-ish format's. A time format hides a day
        // step (13:45 tomorrow still reads 13:45), so ask the serial itself.
        session_new();
        session_set_cell(0, 0, 0, "13:45").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 2, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "13:45");
        session_set_cell(0, 0, 1, "=A3-A1").unwrap();
        assert_eq!(
            super::session_copy_tsv(0, 0, 1, 0, 1).trim(),
            "0",
            "a copied time is the same instant, not two days on"
        );
    }

    /// The asymmetry Excel actually has, and the one a "simplification" would
    /// destroy: a lone *plain* number copies where a lone date steps.
    #[test]
    fn a_lone_plain_number_still_copies() {
        session_new();
        session_set_cell(0, 0, 0, "5").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 3, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "5");
        assert_eq!(session_cell_input(0, 2, 0), "5");
        assert_eq!(session_cell_input(0, 3, 0), "5");

        // Two cells still establish a step, as they always did.
        session_new();
        session_set_cell(0, 0, 0, "1").unwrap();
        session_set_cell(0, 1, 0, "3").unwrap();
        session_fill(0, 0, 0, 1, 0, 0, 0, 3, 0).unwrap();
        assert_eq!(session_cell_input(0, 2, 0), "5");
        assert_eq!(session_cell_input(0, 3, 0), "7");

        // And "fill series" from a single number still steps by one.
        session_new();
        session_set_cell(0, 0, 0, "5").unwrap();
        super::session_fill_mode(0, 0, 0, 0, 0, 0, 0, 2, 0, "series").unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "6");
        assert_eq!(session_cell_input(0, 2, 0), "7");
    }

    /// `Item 1` tiled where Excel gives `Item 2` — a trailing integer in
    /// otherwise identical text continues, and its padding survives.
    #[test]
    fn trailing_integers_in_text_continue() {
        session_new();
        session_set_cell(0, 0, 0, "Item 1").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 2, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "Item 2");
        assert_eq!(session_cell_input(0, 2, 0), "Item 3");

        // Zero padding is part of the text, so it is kept — Q01 → Q02, never Q2.
        session_new();
        session_set_cell(0, 0, 0, "Q01").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 2, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "Q02");
        assert_eq!(session_cell_input(0, 2, 0), "Q03");

        // Width grows naturally when the number outgrows it.
        session_new();
        session_set_cell(0, 0, 0, "Item 9").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 1, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "Item 10");

        // Text with no trailing integer still tiles.
        session_new();
        session_set_cell(0, 0, 0, "Widget").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 2, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "Widget");
        assert_eq!(session_cell_input(0, 2, 0), "Widget");

        // Two cells set the step, and the prefix must match exactly.
        session_new();
        session_set_cell(0, 0, 0, "Q1").unwrap();
        session_set_cell(0, 1, 0, "Q3").unwrap();
        session_fill(0, 0, 0, 1, 0, 0, 0, 3, 0).unwrap();
        assert_eq!(session_cell_input(0, 2, 0), "Q5");
        assert_eq!(session_cell_input(0, 3, 0), "Q7");

        session_new();
        session_set_cell(0, 0, 0, "Item 1").unwrap();
        session_set_cell(0, 1, 0, "Thing 2").unwrap();
        session_fill(0, 0, 0, 1, 0, 0, 0, 3, 0).unwrap();
        assert_eq!(
            session_cell_input(0, 2, 0),
            "Item 1",
            "different prefixes tile"
        );
        assert_eq!(session_cell_input(0, 3, 0), "Thing 2");

        // Filling upward counts down.
        session_new();
        session_set_cell(0, 4, 0, "Item 5").unwrap(); // A5
        session_fill(0, 4, 0, 4, 0, 2, 0, 4, 0).unwrap();
        assert_eq!(session_cell_input(0, 3, 0), "Item 4");
        assert_eq!(session_cell_input(0, 2, 0), "Item 3");

        // Ctrl-drag copies verbatim.
        session_new();
        session_set_cell(0, 0, 0, "Item 1").unwrap();
        super::session_fill_mode(0, 0, 0, 0, 0, 0, 0, 2, 0, "copy").unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "Item 1");
        assert_eq!(session_cell_input(0, 2, 0), "Item 1");

        // A number too large to be an integer is text, not a series — and must
        // not panic or wrap around.
        session_new();
        session_set_cell(0, 0, 0, "'X99999999999999999999").unwrap();
        session_fill(0, 0, 0, 0, 0, 0, 0, 1, 0).unwrap();
        assert_eq!(session_cell_input(0, 1, 0), "'X99999999999999999999");
    }
}

#[cfg(test)]
mod range_list_tests {
    use super::{
        session_add_sheet, session_new, session_set_cell, session_set_list_validation,
        session_set_list_validation_range, session_validation_at, session_validation_error,
    };

    /// The commonest kind of real dropdown, and the one that reached nothing.
    ///
    /// Excel's lists are usually a *range* kept out of the way and maintained on
    /// its own, not an inline CSV. The importer preserved the reference and its
    /// own comment admitted the consequence — "the rule survives even though the
    /// editor cannot offer the dropdown yet" — because both the chevron and the
    /// enforcement gated on the literal `values` being non-empty. So a user
    /// opened their workbook, the dropdowns were gone, and nothing said why.
    #[test]
    fn a_list_backed_by_a_range_offers_and_enforces_its_values() {
        session_new();
        for (i, name) in ["North", "South", "East"].iter().enumerate() {
            session_set_cell(0, i as u32, 5, name).unwrap();
        }
        session_set_list_validation_range(0, 0, 0, 4, 0, "$F$1:$F$3").unwrap();

        assert_eq!(
            session_validation_at(0, 0, 0),
            r#"["North","South","East"]"#,
            "the dropdown lists what the range holds"
        );
        assert_eq!(session_validation_error(0, 0, 0, "South"), "");
        assert!(
            session_validation_error(0, 0, 0, "Westeros").contains("must be one of"),
            "and the rule is enforced, not merely offered"
        );
        // Excel matches a list case-insensitively.
        assert_eq!(session_validation_error(0, 0, 0, "north"), "");
    }

    /// The list is live: it is resolved when asked for, not copied at set time.
    #[test]
    fn editing_the_source_range_changes_the_dropdown() {
        session_new();
        session_set_cell(0, 0, 5, "Draft").unwrap();
        session_set_list_validation_range(0, 0, 0, 0, 0, "$F$1:$F$2").unwrap();
        assert_eq!(session_validation_at(0, 0, 0), r#"["Draft"]"#);

        session_set_cell(0, 1, 5, "Final").unwrap();
        assert_eq!(
            session_validation_at(0, 0, 0),
            r#"["Draft","Final"]"#,
            "adding a row to the source adds an option, as in Excel"
        );
        // And what was rejected a moment ago is now allowed.
        assert_eq!(session_validation_error(0, 0, 0, "Final"), "");
    }

    /// A list may live on another sheet — which is the usual reason to use one.
    #[test]
    fn the_source_range_may_name_another_sheet() {
        session_new();
        session_add_sheet().unwrap();
        session_set_cell(1, 0, 0, "Red").unwrap();
        session_set_cell(1, 1, 0, "Blue").unwrap();
        // Whatever the new sheet is called — the point is that the reference is
        // resolved by name, not that the name is predictable.
        let names: Vec<String> = serde_json::from_str(&super::session_sheet_names()).unwrap();
        let source = format!("{}!$A$1:$A$2", names[1]);
        session_set_list_validation_range(0, 0, 0, 0, 0, &source).unwrap();
        assert_eq!(session_validation_at(0, 0, 0), r#"["Red","Blue"]"#);
    }

    /// Blanks in the source are not options, and an unreadable source refuses
    /// nothing rather than refusing everything.
    #[test]
    fn blanks_are_skipped_and_an_unreadable_source_blocks_nothing() {
        session_new();
        session_set_cell(0, 0, 5, "One").unwrap();
        // F2 left empty; F3 filled — the gap a growing list always has.
        session_set_cell(0, 2, 5, "Three").unwrap();
        session_set_list_validation_range(0, 0, 0, 0, 0, "$F$1:$F$3").unwrap();
        assert_eq!(session_validation_at(0, 0, 0), r#"["One","Three"]"#);

        session_set_list_validation_range(0, 1, 0, 1, 0, "Nowhere!$A$1:$A$2").unwrap();
        assert_eq!(
            session_validation_at(0, 1, 0),
            "null",
            "no list, so no chevron"
        );
        assert_eq!(
            session_validation_error(0, 1, 0, "anything"),
            "",
            "an unreadable source is not a reason to refuse what somebody typed"
        );
    }

    /// The inline form keeps working exactly as it did.
    #[test]
    fn an_inline_list_is_unchanged() {
        session_new();
        session_set_list_validation(0, 0, 0, 0, 0, vec!["Yes".to_owned(), "No".to_owned()])
            .unwrap();
        assert_eq!(session_validation_at(0, 0, 0), r#"["Yes","No"]"#);
        assert!(session_validation_error(0, 0, 0, "Maybe").contains("must be one of"));
    }
}

#[cfg(test)]
mod protection_tests {
    use super::{
        EditOperation, axis_edit, axis_edit_blocked, protection_blocks, session_cell_input,
        session_new, session_set_cell, session_set_cell_protection, session_set_col_width,
        session_set_sheet_protected,
    };

    /// Protection was stored, round-tripped and toggleable — and never
    /// enforced, so a workbook whose author locked its cells opened fully
    /// editable. The default matters as much as the guard: OOXML locks a cell
    /// unless it says otherwise, so an absent flag must read as locked.
    #[test]
    fn a_protected_sheet_refuses_a_locked_cell_and_allows_an_unlocked_one() {
        session_new();
        session_set_cell(0, 0, 0, "before").unwrap();
        session_set_sheet_protected(0, true).unwrap();

        // A1 carries no protection attribute at all, which means locked.
        assert!(
            protection_blocks(0, 0, 0, 0, 0),
            "an unmarked cell defaults to locked"
        );

        // The input cells of a protected sheet are the ones marked unlocked.
        session_set_cell_protection(0, 0, 0, 0, 0, "locked", false).unwrap();
        assert!(!protection_blocks(0, 0, 0, 0, 0));
        session_set_cell(0, 0, 0, "after").unwrap();
        assert_eq!(session_cell_input(0, 0, 0), "after");

        // A block containing one locked cell is refused whole, as in Excel.
        assert!(protection_blocks(0, 0, 0, 1, 1), "B2 is still locked");

        // ...and unprotecting releases everything again.
        session_set_sheet_protected(0, false).unwrap();
        assert!(!protection_blocks(0, 0, 0, 5, 5));
    }

    /// The first column's width in pixels, read back through the binding.
    fn width_px(sheet: usize, col: u32) -> i64 {
        serde_json::from_str::<Vec<i64>>(&super::session_col_px(sheet, col, 1))
            .expect("widths are json")[0]
    }

    /// **Resizing obeys sheet protection.**
    ///
    /// Every write went through `guard_protected` except this one: `edit_axis`
    /// took the sheet index as `_sheet` and dropped it, so a column on a
    /// protected sheet resized happily (`UX-PROT-01`). The unused parameter is
    /// what made it invisible — the signature looked as though it cared.
    ///
    /// A resize is not a cell edit, so the question is not whether the cells
    /// are locked. Excel gates it behind protection's own "Format columns" and
    /// "Format rows" options, and a file that does not mention them has not
    /// granted them.
    #[test]
    fn resizing_obeys_sheet_protection() {
        session_new();
        // Unprotected: nothing is blocked, and the resize lands.
        assert!(!axis_edit_blocked(0, "formatColumns"));
        session_set_col_width(0, 0, 120).expect("an unprotected sheet resizes");
        let before = width_px(0, 0);

        session_set_sheet_protected(0, true).unwrap();
        // The sheet says nothing about formatting, so it has not allowed it.
        assert!(
            axis_edit_blocked(0, "formatColumns"),
            "a protected sheet that never granted formatColumns allowed a resize"
        );
        assert!(axis_edit_blocked(0, "formatRows"));
        // And the binding **honours** it. Asserting only `axis_edit_blocked`
        // above would test the rule and not its use: removing the guard from
        // `edit_axis` leaves that assertion green, which is exactly what a
        // mutation showed. `is_err` rather than `expect_err`, because
        // formatting a `JsError` off-wasm panics.
        // And the path the binding takes **honours** it. Asserting only
        // `axis_edit_blocked` tests the rule and not its use: removing the
        // guard from `axis_edit` leaves that assertion green, which is exactly
        // what a mutation showed.
        assert!(
            axis_edit(
                0,
                "formatColumns",
                EditOperation::SetColumnWidth {
                    sheet: 0,
                    col: 0,
                    width: Some(4000),
                },
            )
            .is_err(),
            "the guard exists and the edit path does not consult it"
        );
        assert_eq!(
            width_px(0, 0),
            before,
            "the refused resize changed the column anyway"
        );

        // And unprotecting releases it again.
        session_set_sheet_protected(0, false).unwrap();
        assert!(!axis_edit_blocked(0, "formatColumns"));
        session_set_col_width(0, 0, 200).expect("unprotected again");
    }

    /// `<protection hidden="1">` keeps the formula out of the formula bar while
    /// the sheet is protected — and only then.
    #[test]
    fn a_hidden_formula_is_withheld_only_while_the_sheet_is_protected() {
        session_new();
        session_set_cell(0, 0, 0, "=1+1").unwrap();
        session_set_cell_protection(0, 0, 0, 0, 0, "hidden", true).unwrap();
        // The engine normalises the formula it stores, so compare with what it
        // round-trips rather than with what was typed.
        let shown = session_cell_input(0, 0, 0);
        assert!(
            shown.starts_with('='),
            "the flag alone hides nothing: {shown}"
        );

        session_set_sheet_protected(0, true).unwrap();
        assert!(
            !session_cell_input(0, 0, 0).starts_with('='),
            "carrying the flag through every save while still showing the \
             formula defeats the only thing it does"
        );
    }
}

#[cfg(test)]
mod page_setup_tests {
    use super::{
        session_new, session_page_setup, session_print_html, session_set_cell,
        session_set_page_setup,
    };
    use crate::view::{HfContext, HfPart, hf_sections};

    /// Every print attribute was carried verbatim with nothing able to change
    /// it, so a sheet imported as landscape could only ever be saved that way.
    #[test]
    fn page_setup_round_trips_through_the_flattened_keys() {
        session_new();
        session_set_page_setup(
            0,
            vec![
                "page.orientation".to_owned(),
                "margins.top".to_owned(),
                "options.gridLines".to_owned(),
            ],
            vec!["landscape".to_owned(), "1.25".to_owned(), "1".to_owned()],
        )
        .unwrap();
        let json = session_page_setup(0);
        assert!(
            json.contains("\"page.orientation\":\"landscape\""),
            "{json}"
        );
        assert!(json.contains("\"margins.top\":\"1.25\""), "{json}");

        // An empty value removes the attribute rather than writing "": OOXML's
        // defaults are meaningful, and `orientation=""` is not `orientation`
        // absent.
        session_set_page_setup(0, vec!["page.orientation".to_owned()], vec![String::new()])
            .unwrap();
        assert!(
            !session_page_setup(0).contains("page.orientation"),
            "an empty value must remove the attribute"
        );
    }

    #[test]
    fn the_printable_page_honours_orientation_margins_and_headings() {
        session_new();
        session_set_cell(0, 0, 0, "Widget").unwrap();
        session_set_page_setup(
            0,
            vec![
                "page.orientation".to_owned(),
                "page.paperSize".to_owned(),
                "margins.left".to_owned(),
                "options.headings".to_owned(),
            ],
            vec![
                "landscape".to_owned(),
                "9".to_owned(),
                "1.5".to_owned(),
                "1".to_owned(),
            ],
        )
        .unwrap();
        let html = session_print_html(0);
        assert!(html.contains("size:A4 landscape"), "{html}");
        assert!(html.contains("1.5in"), "{html}");
        // Headings on means the A/1 strips are printed, so the letter is there.
        assert!(html.contains("<th>A</th>"), "{html}");
        assert!(html.contains("Widget"), "{html}");
    }

    /// Field codes are markup, not text, and they are **substituted** rather
    /// than dropped.
    ///
    /// `strip_hf_codes` turned every one into a space, so `&P` — the code the
    /// dialog's own placeholder text advertises — could not put a page number
    /// on the paper at all.
    #[test]
    fn header_field_codes_are_substituted_into_their_three_sections() {
        let ctx = HfContext {
            sheet: "Q3",
            file: "",
            now: None,
        };
        // Section codes place the text; the page number survives as a token.
        let [left, centre, right] = hf_sections("&LSales&RPage &P of &N", &ctx);
        assert_eq!(left, vec![HfPart::Text("Sales".to_owned())]);
        assert!(centre.is_empty(), "{centre:?}");
        assert_eq!(
            right,
            vec![
                HfPart::Text("Page ".to_owned()),
                HfPart::PageNumber,
                HfPart::Text(" of ".to_owned()),
                HfPart::PageCount,
            ]
        );

        // Font and point-size codes are consumed; the text between survives.
        let [_, centre, _] = hf_sections("&\"Arial,Bold\"&14Report", &ctx);
        assert_eq!(centre, vec![HfPart::Text("Report".to_owned())]);

        // `&&` is a literal ampersand and must survive; `&A` is the sheet name.
        let [_, centre, _] = hf_sections("Profit && Loss - &A", &ctx);
        assert_eq!(centre, vec![HfPart::Text("Profit & Loss - Q3".to_owned())]);

        assert!(hf_sections("", &ctx).iter().all(Vec::is_empty));
    }

    /// The page number reaches the printed document as a CSS page counter in an
    /// `@page` margin box, which is the one place a browser can count pages.
    #[test]
    fn the_page_number_is_emitted_as_a_page_counter() {
        session_new();
        session_set_cell(0, 0, 0, "Widget").unwrap();
        session_set_page_setup(
            0,
            vec!["hf.oddFooter".to_owned()],
            vec!["&CPage &P of &N".to_owned()],
        )
        .unwrap();
        let html = session_print_html(0);
        assert!(
            html.contains("@bottom-center{content:\"Page \" counter(page) \" of \" counter(pages)"),
            "{html}"
        );
    }
}

#[cfg(test)]
mod print_fidelity_tests {
    use super::{
        session_hide_cols, session_merge_cells, session_new, session_print_html,
        session_set_border, session_set_cell, session_set_col_width, session_set_page_setup,
        session_set_print_area, session_set_row_height,
    };

    /// The printout is the deliverable for a lot of users, and it was not the
    /// sheet: `<table style="table-layout:fixed">` with no `<col>` anywhere, so
    /// every column printed at the same width whatever the screen showed
    /// (`docs/12` §3.17, switching-blocker #5).
    #[test]
    fn column_widths_and_row_heights_reach_the_printed_table() {
        session_new();
        session_set_cell(0, 0, 0, "wide").unwrap();
        session_set_cell(0, 0, 1, "narrow").unwrap();
        session_set_col_width(0, 0, 200).unwrap();
        session_set_col_width(0, 1, 40).unwrap();
        session_set_row_height(0, 0, 33).unwrap();
        let html = session_print_html(0);
        assert!(
            html.contains(
                "<colgroup><col style=\"width:200px\"><col style=\"width:40px\"></colgroup>"
            ),
            "{html}"
        );
        assert!(html.contains("height:33px"), "{html}");
    }

    /// Merges printed as separate cells: the generator emitted no `colspan` or
    /// `rowspan` at all, so a merged title printed as one cell of text beside
    /// the empty cells it had swallowed on screen.
    #[test]
    fn merges_print_as_colspan_and_rowspan() {
        session_new();
        session_set_cell(0, 0, 0, "Title").unwrap();
        session_set_cell(0, 2, 0, "below").unwrap();
        session_merge_cells(0, 0, 0, 1, 2).unwrap();
        let html = session_print_html(0);
        assert!(html.contains("colspan=\"3\""), "{html}");
        assert!(html.contains("rowspan=\"2\""), "{html}");
        // The cells the merge covers must not also be emitted, or the row has
        // more cells in it than the table has columns.
        assert_eq!(html.matches("<td").count(), 4, "{html}");
    }

    /// A span counts the lines that *print*, not the lines the model holds.
    ///
    /// Both halves of this are ways to emit a row with more cells in it than
    /// the table has columns, which renders as a staircase rather than as a
    /// table: a merge over a hidden column, and a merge whose top-left the
    /// print area clips away so nothing is left to hang the span on.
    #[test]
    fn a_span_counts_printed_lines_not_model_lines() {
        session_new();
        session_set_cell(0, 0, 0, "Banner").unwrap();
        session_set_cell(0, 1, 0, "a").unwrap();
        session_set_cell(0, 1, 1, "b").unwrap();
        session_set_cell(0, 1, 2, "c").unwrap();
        session_merge_cells(0, 0, 0, 0, 2).unwrap();
        session_hide_cols(0, 1, 1).unwrap();
        let html = session_print_html(0);
        assert!(
            html.contains("colspan=\"2\""),
            "a hidden column narrows the span: {html}"
        );
        // Two printed columns, and the banner row must hold exactly one cell.
        assert_eq!(html.matches("<col ").count(), 2, "{html}");
        assert_eq!(html.matches("<td").count(), 3, "{html}");

        // Now clip the merge's top-left away with a print area. The first
        // corner that still prints has to carry the span, or the row loses a
        // cell and the table renders as a staircase.
        session_new();
        session_set_cell(0, 0, 0, "Banner").unwrap();
        session_set_cell(0, 1, 1, "b").unwrap();
        session_set_cell(0, 1, 2, "c").unwrap();
        session_merge_cells(0, 0, 0, 0, 2).unwrap();
        session_set_print_area(0, 0, 1, 1, 2).unwrap();
        let html = session_print_html(0);
        assert!(html.contains("colspan=\"2\""), "{html}");
        assert_eq!(html.matches("<col ").count(), 2, "{html}");
        assert_eq!(html.matches("<td").count(), 3, "{html}");
    }

    /// Cell borders did not print. The only border rule was a blanket
    /// `td,th{border:1px solid #b0b0b0}` behind the gridlines switch, so a
    /// styled table printed as either a uniform grey grid or nothing.
    #[test]
    fn cell_borders_are_carried_into_the_printed_cell() {
        session_new();
        session_set_cell(0, 0, 0, "boxed").unwrap();
        session_set_border(0, 0, 0, 0, 0, "all", "medium", "FF0000").unwrap();
        let html = session_print_html(0);
        assert!(html.contains("border-top:2px solid #FF0000;"), "{html}");
        assert!(html.contains("border-bottom:2px solid #FF0000;"), "{html}");
        assert!(html.contains("border-left:2px solid #FF0000;"), "{html}");
        assert!(html.contains("border-right:2px solid #FF0000;"), "{html}");
    }

    /// The three scale controls in Page setup changed the saved file and
    /// nothing else: the emitted CSS was only `@page{size:…;margin:…}`.
    #[test]
    fn the_scale_percent_is_applied_to_the_printed_table() {
        session_new();
        session_set_cell(0, 0, 0, "x").unwrap();
        session_set_page_setup(0, vec!["page.scale".to_owned()], vec!["70".to_owned()]).unwrap();
        let html = session_print_html(0);
        assert!(html.contains("table{zoom:0.7}"), "{html}");
    }

    /// Fit-to-width is arithmetic over the grid against the printable area, so
    /// only the engine can answer it — CSS has no fit-to-page primitive.
    ///
    /// Twenty 200 px columns is 4000 px of grid; Letter portrait less 0.7 in
    /// margins leaves 7.1 in, which is 681.6 px. The scale is that ratio.
    #[test]
    fn fit_to_one_page_wide_shrinks_the_table_to_the_printable_width() {
        session_new();
        for c in 0..20u32 {
            session_set_cell(0, 0, c, "x").unwrap();
            session_set_col_width(0, c, 200).unwrap();
        }
        session_set_page_setup(
            0,
            vec![
                "setupPr.fitToPage".to_owned(),
                "page.fitToWidth".to_owned(),
                "page.fitToHeight".to_owned(),
            ],
            vec!["1".to_owned(), "1".to_owned(), "0".to_owned()],
        )
        .unwrap();
        let html = session_print_html(0);
        assert!(html.contains("table{zoom:0.17}"), "{html}");

        // Fit-to-page only shrinks. A sheet that already fits is left alone,
        // not blown up to fill the paper.
        session_new();
        session_set_cell(0, 0, 0, "x").unwrap();
        session_set_page_setup(
            0,
            vec![
                "setupPr.fitToPage".to_owned(),
                "page.fitToWidth".to_owned(),
                "page.fitToHeight".to_owned(),
            ],
            vec!["1".to_owned(), "1".to_owned(), "0".to_owned()],
        )
        .unwrap();
        assert!(
            !session_print_html(0).contains("zoom:"),
            "fit-to-page enlarged a sheet that fits"
        );
    }

    /// A header is workbook text and reaches a `<style>` element, where
    /// `push_html_escaped` is no defence: `&lt;` inside a stylesheet is four
    /// literal characters. `</style>` in a header would close the sheet and let
    /// the rest run as markup, in a popup carrying the editor's origin.
    #[test]
    fn a_header_cannot_close_the_style_element() {
        session_new();
        session_set_cell(0, 0, 0, "x").unwrap();
        session_set_page_setup(
            0,
            vec!["hf.oddHeader".to_owned()],
            vec!["&C</style><img src=x onerror=alert(1)>".to_owned()],
        )
        .unwrap();
        let html = session_print_html(0);
        let style = html.split("<style>").nth(1).unwrap_or_default();
        let style = style.split("</style>").next().unwrap_or_default();
        assert!(style.contains("@top-center{content:"), "{html}");
        assert!(
            !style.contains('<'),
            "a raw `<` survived into the stylesheet: {style}"
        );
        assert!(!html.contains("onerror=alert(1)>"), "{html}");
    }
}

#[cfg(test)]
mod print_scope_tests {
    use super::{
        session_clear_print_area, session_new, session_print_html, session_print_scope,
        session_set_cell, session_set_print_area, session_set_print_title_rows,
    };

    fn grid() {
        session_new();
        for r in 0..6u32 {
            for c in 0..4u32 {
                session_set_cell(0, r, c, &format!("r{r}c{c}")).unwrap();
            }
        }
    }

    /// `Print_Area` and `Print_Titles` are ordinary sheet-scoped defined names
    /// — that is the whole mechanism. They were carried verbatim with no way to
    /// set one and no effect on anything printed.
    #[test]
    fn a_print_area_narrows_what_prints_and_clearing_it_restores_the_rest() {
        grid();
        session_set_print_area(0, 1, 1, 2, 2).unwrap();
        let scope = session_print_scope(0);
        assert!(scope.contains("$B$2:$C$3"), "{scope}");

        let html = session_print_html(0);
        assert!(html.contains("r1c1"), "inside the area: {html}");
        assert!(!html.contains("r0c0"), "outside it, above-left: {html}");
        assert!(!html.contains("r5c3"), "outside it, below-right: {html}");

        session_clear_print_area(0).unwrap();
        let all = session_print_html(0);
        assert!(all.contains("r0c0") && all.contains("r5c3"), "{all}");
    }

    /// Repeated rows go in `<thead>`, which the browser puts at the top of
    /// every page it breaks onto — and must not also appear in the body, or the
    /// first page shows them twice.
    #[test]
    fn title_rows_are_a_thead_and_are_not_repeated_in_the_body() {
        grid();
        session_set_print_title_rows(0, 0, 0).unwrap();
        let html = session_print_html(0);
        let head = html.find("<thead>").expect("a thead");
        let head_end = html.find("</thead>").expect("a closed thead");
        assert!(html[head..head_end].contains("r0c0"), "{html}");
        assert_eq!(
            html.matches("r0c0").count(),
            1,
            "the title row must not also be in the body: {html}"
        );

        // Clearing takes the thead away again.
        session_set_print_title_rows(0, 1, 0).unwrap();
        assert!(!session_print_html(0).contains("<thead>"));
    }
}

#[cfg(test)]
mod sort_state_tests {
    use super::{session_new, session_set_cell, session_sort_range};

    /// Excel writes `<sortState>` so its own dialog can reopen showing the keys
    /// that were used. Sorting here left no trace, so a file sorted in this app
    /// opened in Excel claiming it had never been sorted.
    #[test]
    fn sorting_records_what_it_sorted() {
        session_new();
        for (i, v) in ["3", "1", "2"].iter().enumerate() {
            session_set_cell(0, i as u32, 0, v).unwrap();
        }
        session_sort_range(0, 0, 0, 2, 0, 0, false).unwrap();

        let state = super::with_session(|s| s.workbook().sheets[0].sort_state.clone())
            .flatten()
            .expect("a sort leaves a record");
        assert_eq!(state.attrs.get("ref").map(String::as_str), Some("A1:A3"));
        assert_eq!(state.conditions.len(), 1);
        // Ascending is the schema default and is written by omission, so a
        // descending sort is the one that carries the attribute.
        assert_eq!(
            state.conditions[0].get("descending").map(String::as_str),
            Some("1")
        );
    }
}

#[cfg(test)]
mod page_break_tests {
    use super::{
        session_new, session_page_breaks, session_print_html, session_set_cell,
        session_toggle_page_break,
    };

    /// `<brk id>` is one-based, matching the row number a user sees, while
    /// everything else here is zero-based. Getting that boundary wrong puts
    /// every break one row out.
    #[test]
    fn a_break_toggles_and_reports_zero_based() {
        session_new();
        session_toggle_page_break(0, 4, 2).unwrap();
        assert_eq!(session_page_breaks(0), r#"{"rows":[4],"cols":[2]}"#);

        // The same command again removes it.
        session_toggle_page_break(0, 4, 2).unwrap();
        assert_eq!(session_page_breaks(0), r#"{"rows":[],"cols":[]}"#);

        // A break before the first line is the page edge; Excel does not write
        // one, and nor should this.
        session_toggle_page_break(0, 0, 0).unwrap();
        assert_eq!(session_page_breaks(0), r#"{"rows":[],"cols":[]}"#);

        // u32::MAX means "not this axis", so a whole-row selection sets no
        // column break.
        session_toggle_page_break(0, 3, u32::MAX).unwrap();
        assert_eq!(session_page_breaks(0), r#"{"rows":[3],"cols":[]}"#);
    }

    #[test]
    fn a_row_break_starts_a_new_printed_page() {
        session_new();
        for r in 0..4u32 {
            session_set_cell(0, r, 0, &format!("r{r}")).unwrap();
        }
        session_toggle_page_break(0, 2, u32::MAX).unwrap();
        let html = session_print_html(0);
        assert_eq!(
            html.matches("break-before:page").count(),
            1,
            "exactly the one break: {html}"
        );
        // ...and it is on the row the break sits before, not the one after.
        let at = html.find("break-before:page").expect("a break");
        assert!(html[at..].contains("r2"), "{html}");
    }
}

#[cfg(test)]
mod base64_tests {
    use super::base64_encode;

    /// The padding cases are where a hand-written encoder goes wrong, and a
    /// wrong `data:` URL is a picture the browser silently refuses to decode.
    #[test]
    fn base64_pads_every_remainder() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        // High bytes must not be sign-extended or mangled.
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }
}

#[cfg(test)]
mod collab_tests {
    //! The collaboration seam, exercised the way a browser client would.
    //!
    //! These bindings were the missing piece: everything under them was gated
    //! and reachable from Rust and from nowhere else, so the browser had no way
    //! to join a session even though the engine could.

    use super::*;

    fn fresh(client: u64, revision: u64) {
        session_new();
        collab_begin(client as f64, revision as f64);
    }

    #[test]
    fn a_session_starts_from_the_revision_the_server_named() {
        // Everyone in a session must start from the same revision. A client
        // that guessed would rebase against a history it never saw.
        fresh(1, 9);
        assert!(collab_active());
        assert_eq!(collab_revision(), 9.0);
        collab_end();
        assert!(!collab_active());
    }

    #[test]
    fn local_edits_come_out_as_one_chunk_at_a_time() {
        // One in flight by design: a client with two outstanding chunks cannot
        // say which the server's acknowledgement was for.
        fresh(1, 0);
        assert_eq!(collab_flush(), "", "nothing to send yet");

        session_set_cell(0, 0, 0, "42").unwrap();
        let first = collab_flush();
        assert!(first.contains("\"seq\""), "got {first}");

        session_set_cell(0, 1, 0, "43").unwrap();
        // Sent without waiting for the first to be acknowledged, and chained to
        // it: only the first chunk after joining knows an absolute revision.
        // This asserted the opposite before ADR-016, which is the rule that
        // change removed.
        let second = collab_flush();
        assert!(second.contains("\"chained\""), "got {second}");

        collab_acknowledge(2.0, 2.0);
        assert_eq!(collab_revision(), 2.0);
        assert_eq!(
            collab_flush(),
            "",
            "and with both settled there is nothing left to send"
        );
    }

    #[test]
    fn a_resend_reuses_its_sequence_number_so_a_reconnect_is_safe() {
        // The server answers `Duplicate` rather than applying it twice, which
        // is what makes reconnecting safe rather than merely likely to work.
        fresh(1, 0);
        session_set_cell(0, 0, 0, "42").unwrap();
        let sent = collab_flush();
        // An array since ADR-016: several chunks may be outstanding, so a
        // reconnect may have several to send again.
        let again: Vec<serde_json::Value> =
            serde_json::from_str(&collab_resend()).expect("a list of messages");
        assert_eq!(again.len(), 1);
        // Compared as values, not as text: `serde_json::Value` sorts its keys,
        // so a string comparison here would fail on field order alone and say
        // nothing about the content.
        assert_eq!(
            again[0],
            serde_json::from_str::<serde_json::Value>(&sent).unwrap(),
            "the same chunk, so the server recognises it rather than applying it twice"
        );

        // And a second, chained chunk comes back with it, in order.
        session_set_cell(0, 1, 0, "43").unwrap();
        collab_flush();
        let again: Vec<serde_json::Value> =
            serde_json::from_str(&collab_resend()).expect("a list of messages");
        assert_eq!(again.len(), 2, "both outstanding chunks");
        assert_eq!(again[0]["seq"], 1, "oldest first");
        assert_eq!(again[1]["seq"], 2);
        assert!(
            again[0]["base"].get("revision").is_some(),
            "only the first names a revision: {}",
            again[0]["base"]
        );
        assert_eq!(again[1]["base"], "chained");
    }

    #[test]
    fn two_participants_editing_different_cells_both_end_up_with_both_edits() {
        // The whole point, through the bindings a browser would call. One
        // engine plays each participant in turn, exchanging wire operations the
        // way a server would relay them.
        fresh(1, 0);
        session_set_cell(0, 0, 0, "mine").unwrap();
        let from_a: casual_calc_transaction::session::Submission =
            serde_json::from_str(&collab_flush()).expect("a submission");
        let a_saw = session_cells(0, 0, 0, 1, 0);

        // The other participant, from the same starting revision.
        fresh(2, 0);
        session_set_cell(0, 1, 0, "theirs").unwrap();
        let from_b: casual_calc_transaction::session::Submission =
            serde_json::from_str(&collab_flush()).expect("a submission");

        // B receives A's edit, ordered first.
        for op in &from_a.ops {
            collab_receive(&serde_json::to_string(op).unwrap(), 1.0).expect("applied");
        }
        collab_acknowledge(1.0, 2.0);
        let b_final = session_cells(0, 0, 0, 1, 0);

        // And A receives B's, ordered second.
        fresh(1, 0);
        session_set_cell(0, 0, 0, "mine").unwrap();
        collab_flush();
        collab_acknowledge(1.0, 1.0);
        for op in &from_b.ops {
            collab_receive(&serde_json::to_string(op).unwrap(), 2.0).expect("applied");
        }
        let a_final = session_cells(0, 0, 0, 1, 0);

        assert!(a_saw.contains("mine"));
        assert_eq!(
            a_final, b_final,
            "the two participants converged on different orders of the same edits"
        );
        assert!(a_final.contains("mine") && a_final.contains("theirs"));
    }

    /// **A draft is presence, and typing must leave no trace of itself in the
    /// document, the undo history or the applied log.**
    ///
    /// The line ADR-011 draws, asserted at the seam where it would be crossed.
    /// The tempting implementation of "show me what they are typing" is to
    /// write the cell on every keystroke and let the transform sort it out —
    /// which converges, and is also wrong in every other way: it fills the undo
    /// stack with half-words, sends an operation per keypress for everybody
    /// else to transform and apply, and commits text the author may be about to
    /// abandon. Every one of those is caught by `collab_flush()` staying empty.
    ///
    /// Checked after **each** keystroke rather than at the end, because a
    /// version that submitted the draft and cleared it again would pass a
    /// single check at the end.
    #[test]
    fn a_draft_is_presence_and_never_reaches_the_document_the_history_or_the_log() {
        fresh(1, 0);
        for typed in ["=", "=S", "=SU", "=SUM(A1:A9"] {
            let message = collab_presence(0, &[3, 1, 3, 1], 3, 1, Some(typed.to_owned()));
            let parsed: ClientMessage =
                serde_json::from_str(&message).expect("a presence message: {message}");
            let ClientMessage::Presence { editing, .. } = parsed else {
                panic!("a draft must travel as presence, got {message}");
            };
            assert_eq!(
                editing.as_ref().map(|d| d.text.as_str()),
                Some(typed),
                "the draft carries the text, which is what was asked for"
            );
            assert_eq!(
                collab_flush(),
                "",
                "typing produced an operation to submit: {typed}"
            );
        }

        // Nothing in the document...
        assert_eq!(session_cell_input(0, 3, 1), "", "the cell is still empty");
        // ...nothing to undo, so the author's own history is untouched...
        assert!(!session_can_undo(), "a draft entered the undo history");
        // ...and nothing owed to the server.
        assert!(!collab_unacknowledged(), "a draft is outstanding work");

        // And committing *does* produce a chunk — so the assertions above are
        // about drafts being ephemeral, not about the session being inert.
        session_set_cell(0, 3, 1, "=SUM(A1:A9)").unwrap();
        assert!(
            collab_flush().contains("\"seq\""),
            "the committed value is what travels as an operation"
        );
    }

    /// A draft from a hostile or merely careless client is cut back before it
    /// is put into a message.
    #[test]
    fn a_draft_is_bounded_before_it_leaves_this_engine() {
        fresh(1, 0);
        let typed = "x".repeat(50_000);
        let message = collab_presence(0, &[0, 0, 0, 0], 0, 0, Some(typed));
        let ClientMessage::Presence {
            editing: Some(draft),
            ..
        } = serde_json::from_str(&message).expect("a presence message")
        else {
            panic!("expected a draft");
        };
        assert_eq!(
            draft.text.chars().count(),
            casual_calc_transaction::protocol::Draft::MAX_TEXT
        );
    }

    /// Not typing is the absent field, which is how Escape reaches everybody.
    #[test]
    fn a_participant_who_stopped_typing_says_so_by_carrying_no_draft() {
        fresh(1, 0);
        let message = collab_presence(0, &[3, 1, 3, 1], 3, 1, None);
        assert!(
            !message.contains("editing"),
            "an abandoned edit must leave nothing behind: {message}"
        );
    }

    #[test]
    fn a_remote_edit_recalculates_what_depends_on_it() {
        // A remote edit changes values, so it must get the same recalculation a
        // local one does — otherwise a formula shows a stale answer until its
        // own cell is touched.
        fresh(1, 0);
        session_set_cell(0, 0, 0, "2").unwrap();
        session_set_cell(0, 1, 0, "=A1*10").unwrap();
        assert!(session_cells(0, 1, 0, 1, 0).contains("20"));

        // Somebody else changes A1.
        let other = {
            session_new();
            let mut scratch = ClientSession::new(casual_calc_transaction::session::ClientId(2), 0);
            let _ = &mut scratch;
            session_set_cell(0, 0, 0, "5").unwrap();
            collab_begin(2.0, 0.0);
            session_set_cell(0, 0, 0, "5").unwrap();
            collab_flush()
        };
        let submission: casual_calc_transaction::session::Submission =
            serde_json::from_str(&other).expect("a submission");

        fresh(1, 0);
        session_set_cell(0, 0, 0, "2").unwrap();
        session_set_cell(0, 1, 0, "=A1*10").unwrap();
        collab_flush();
        collab_acknowledge(1.0, 1.0);
        for op in &submission.ops {
            collab_receive(&serde_json::to_string(op).unwrap(), 2.0).expect("applied");
        }
        assert!(
            session_cells(0, 1, 0, 1, 0).contains("50"),
            "the formula did not recalculate after a remote edit: {}",
            session_cells(0, 1, 0, 1, 0)
        );
    }
}

#[cfg(test)]
mod snapshot_boundary_tests {
    //! The snapshot the server hands a joining participant must be one this
    //! engine can load.
    //!
    //! Two crates, two calls, and nothing that would notice them drifting: the
    //! server captures with `to_snapshot` and this loads with `from_snapshot`.
    //! Written as bare `serde_json` first, which round-trips a `Workbook`
    //! perfectly and skips the `SCHEMA_VERSION` the format carries — so it
    //! would have worked until the schema moved, then failed in a browser, on
    //! somebody's document.

    use super::*;

    #[test]
    fn a_snapshot_written_the_way_the_server_writes_one_loads_here() {
        session_new();
        session_set_cell(0, 0, 0, "before").unwrap();

        // Exactly what `DocumentSession::join` produces: `Snapshot::capture`
        // calls `Workbook::to_snapshot`.
        let bytes = session_snapshot().expect("captured");

        session_new();
        session_set_cell(0, 0, 0, "something else").unwrap();
        session_load_snapshot(&bytes).expect("the server's snapshot must load");
        assert!(session_cells(0, 0, 0, 0, 0).contains("before"));
    }
}

#[cfg(test)]
mod retained_part_tests {
    use super::{SESSION, WorkbookSession, session_delete_chart, session_undo, set_session};
    use casual_calc_model::{
        CellRange, CellRef, ChartKind, ChartView, Emu, Id, RetainedPart, Sheet, SheetId, Workbook,
    };

    const PART: &str = "xl/charts/chart1.xml";

    /// A workbook holding one imported chart — one the model does not fully
    /// describe, so its original XML is retained and written back verbatim.
    fn with_an_imported_chart() -> Workbook {
        let mut workbook = Workbook::new(Id::from_parts(0x5742, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(0x5348, 1)), "Sheet1");
        sheet.charts.push(ChartView {
            id: 1,
            anchor: CellRange::new(CellRef::new(0, 0), CellRef::new(9, 5)),
            from_offset: Emu { x: 0, y: 0 },
            to_offset: Emu { x: 0, y: 0 },
            kind: ChartKind::Bar,
            title: "Revenue".to_owned(),
            series: Vec::new(),
            legend: None,
            x_title: String::new(),
            y_title: String::new(),
            part: Some(PART.to_owned()),
        });
        workbook.sheets.push(sheet);
        workbook.retained_parts.push(RetainedPart {
            path: PART.to_owned(),
            bytes: b"<chartSpace/>".to_vec(),
            content_type: None,
        });
        workbook
    }

    fn retains(workbook: &Workbook) -> bool {
        workbook.retained_parts.iter().any(|p| p.path == PART)
    }

    /// **Deleting an imported chart reaches the other browser.**
    ///
    /// Two things happen on a chart delete: the sheet's chart list changes,
    /// which travels as `SetSheetMetadata`, and the chart's retained part is
    /// dropped, which went straight to the workbook through `workbook_mut` and
    /// so was recorded in no operation at all.
    ///
    /// A replica therefore keeps a chart part whose chart is gone. Which copy
    /// of the file that produces depends on **which node happens to save** —
    /// the deleting one writes a coherent package, the other writes the deleted
    /// chart's XML back into it. That is silent divergence, and the retained
    /// part is exactly the thing the model cannot reconstruct if it is wrong.
    #[test]
    fn deleting_an_imported_chart_reaches_a_replica() {
        let mut replica = with_an_imported_chart();
        let mut session = WorkbookSession::from_workbook(with_an_imported_chart());
        // Without this the outgoing log is off and `take_applied` returns
        // nothing — which fails this test for the wrong reason entirely, by
        // sending the replica no operations at all rather than the wrong ones.
        session.record_applied();
        set_session(session);

        session_delete_chart(0, 0).expect("deleted");

        // Everything this client would send to the server.
        let sent = SESSION.with(|cell| {
            cell.borrow_mut()
                .as_mut()
                .expect("a session")
                .take_applied()
        });
        assert!(!sent.is_empty(), "the delete produced no operation to send");
        for op in sent {
            casual_calc_transaction::apply(&mut replica, op).expect("the replica applies it");
        }

        let here = SESSION.with(|cell| {
            cell.borrow()
                .as_ref()
                .expect("a session")
                .workbook()
                .clone()
        });

        assert!(!retains(&here), "the deleting client kept the part");
        assert!(
            !retains(&replica),
            "the replica still holds the deleted chart's part: whichever node saves \
             decides what the file contains"
        );
    }

    /// **Undoing a chart deletion brings the chart's bytes back.**
    ///
    /// The part is the chart: the model does not describe one completely enough
    /// to rebuild, which is why the original XML is retained at all. Dropping it
    /// outside the operation meant undo restored the chart's *entry* and not the
    /// chart — the sheet listed a chart whose part had been destroyed, and no
    /// further undo could reach it. Silent, and unrecoverable in the session it
    /// happened in.
    #[test]
    fn undoing_a_chart_deletion_restores_its_retained_bytes() {
        let mut session = WorkbookSession::from_workbook(with_an_imported_chart());
        session.record_applied();
        set_session(session);

        session_delete_chart(0, 0).expect("deleted");
        let after_delete = SESSION.with(|cell| {
            cell.borrow()
                .as_ref()
                .expect("a session")
                .workbook()
                .clone()
        });
        assert!(!retains(&after_delete), "the delete did not drop the part");
        assert!(after_delete.sheets[0].charts.is_empty());

        session_undo().expect("undone");

        let after_undo = SESSION.with(|cell| {
            cell.borrow()
                .as_ref()
                .expect("a session")
                .workbook()
                .clone()
        });
        assert_eq!(
            after_undo.sheets[0].charts.len(),
            1,
            "the chart did not come back"
        );
        assert!(
            retains(&after_undo),
            "the chart came back without the bytes that are the chart"
        );
        assert_eq!(
            after_undo.retained_parts[0].bytes,
            b"<chartSpace/>".to_vec(),
            "restored, but not the same bytes"
        );
    }
}

#[cfg(test)]
mod paste_widths_tests {
    use super::{
        session_clip_copy, session_clip_paste_mode, session_col_width, session_new,
        session_set_cell, session_set_col_width,
    };

    /// **Paste Special carries column widths, and only when asked** (`UX-CLIP-02`).
    ///
    /// Reported from a running stack as "copy-paste does not keep width and
    /// height". It does not, deliberately: a plain paste that reshaped the
    /// sheet it landed in would move columns somebody else's data sits under,
    /// and Excel's plain `Ctrl+V` does not do it either. What Excel *does*
    /// have is this — an explicit option — and now so does this.
    #[test]
    fn a_widths_paste_carries_the_source_columns_and_leaves_the_cells_alone() {
        session_new();
        session_set_cell(0, 0, 0, "wide").unwrap();
        session_set_col_width(0, 0, 300).unwrap();
        let source = session_col_width(0, 0);
        assert!(source > 0.0, "the source column has an explicit width");

        // Somewhere else, at the default width and with content of its own.
        session_set_cell(0, 0, 4, "keep me").unwrap();
        let before = session_col_width(0, 4);
        assert_ne!(
            before, source,
            "the destination starts at a different width"
        );

        session_clip_copy(0, 0, 0, 0, 0, false);
        session_clip_paste_mode(0, 0, 4, "widths").unwrap();

        assert_eq!(
            session_col_width(0, 4),
            source,
            "the column width did not travel"
        );
        // **And nothing else did.** A widths paste that also wrote the cell
        // would be an ordinary paste wearing a different name.
        assert_eq!(
            super::session_cell_input(0, 0, 4),
            "keep me",
            "a widths paste overwrote the destination's contents"
        );
    }

    /// A plain paste still does *not* carry them, which is the behaviour the
    /// explicit option exists to leave alone.
    #[test]
    fn an_ordinary_paste_does_not_reshape_the_sheet_it_lands_in() {
        session_new();
        session_set_cell(0, 0, 0, "wide").unwrap();
        session_set_col_width(0, 0, 300).unwrap();
        let before = session_col_width(0, 4);

        session_clip_copy(0, 0, 0, 0, 0, false);
        session_clip_paste_mode(0, 0, 4, "all").unwrap();

        assert_eq!(
            session_col_width(0, 4),
            before,
            "a plain paste moved a column the person did not ask it to"
        );
    }

    /// A column at the sheet default has no width to carry, and writing the
    /// default onto the destination would pin a column that was following it.
    #[test]
    fn a_source_column_with_no_explicit_width_carries_nothing() {
        session_new();
        session_set_cell(0, 0, 0, "plain").unwrap();
        session_set_col_width(0, 4, 300).unwrap();
        let destination = session_col_width(0, 4);

        session_clip_copy(0, 0, 0, 0, 0, false);
        session_clip_paste_mode(0, 0, 4, "widths").unwrap();

        assert_eq!(
            session_col_width(0, 4),
            destination,
            "a default-width source overwrote a destination that had been set"
        );
    }
}

/// Refuse an edit that a protected sheet forbids.
///
/// Protection was stored, round-tripped and toggleable — and never enforced, so
/// a workbook whose author locked its formulas opened fully editable. In OOXML
/// a cell is locked *by default*: `<protection locked="0">` is what marks the
/// input cells, so an absent flag means locked, and reading it the other way
/// round would unlock the whole sheet.
///
/// Like Excel, this is a guard on user actions rather than on the data: undo
/// and redo still replay edits made before the sheet was protected.
pub(crate) fn guard_protected(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    if protection_blocks(sheet, r0, c0, r1, c1) {
        return Err(JsError::new(
            "this sheet is protected — unprotect it to change locked cells",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod column_stats_tests {
    use super::{
        StatsLimits, column_stats, session_column_stats, session_new, session_set_cell,
        with_session,
    };
    use serde_json::Value;

    fn stats(r0: u32, c0: u32, r1: u32, c1: u32) -> Value {
        let json = session_column_stats(0, r0, c0, r1, c1);
        serde_json::from_str(&json).unwrap_or_else(|why| panic!("not JSON ({why}): {json}"))
    }

    fn set(row: u32, input: &str) {
        session_set_cell(0, row, 0, input).unwrap();
    }

    /// The wire shape itself, pinned.
    ///
    /// The host reads these key names; renaming one is a silent break that no
    /// value-level assertion above would notice.
    #[test]
    fn the_json_shape_is_what_the_host_reads() {
        session_new();
        set(0, "10");
        set(1, "'007");
        set(3, "20");
        assert_eq!(
            session_column_stats(0, 0, 0, 3, 0),
            concat!(
                r#"{"rows":4,"cols":1,"cells":4,"count":3,"empty":1,"unique":3,"#,
                r#""uniqueExact":true,"truncated":false,"#,
                r#""types":{"number":2,"date":0,"text":1,"numberAsText":1,"#,
                r#""boolean":0,"error":0,"formula":0},"errors":{},"#,
                r#""numeric":{"count":2,"sum":30.0,"avg":15.0,"median":15.0,"#,
                r#""min":10.0,"max":20.0,"stdev":7.0710678118654755,"stdevp":5.0},"#,
                r#""frequency":[{"value":"10","type":"number","count":1},"#,
                r#"{"value":"20","type":"number","count":1},"#,
                r#"{"value":"007","type":"text","count":1}],"#,
                r#""frequencyOther":{"values":0,"count":0}}"#,
            )
        );
    }

    /// **A blank is not a zero.**
    ///
    /// The whole reason a stats panel is opened on a column somebody has just
    /// been sent is to see what is *not* there. Averaging blanks as zeroes turns
    /// 15 into 6 and hides the gap that caused it, so empties are counted on
    /// their own line and excluded from every aggregate — as Excel and Sheets
    /// both do.
    #[test]
    fn blanks_are_counted_apart_and_never_average_as_zero() {
        session_new();
        set(0, "10");
        set(2, "20");

        let v = stats(0, 0, 4, 0);
        assert_eq!(v["rows"], 5);
        assert_eq!(v["cells"], 5);
        assert_eq!(v["count"], 2, "non-empty cells");
        assert_eq!(v["empty"], 3);
        assert_eq!(v["unique"], 2);
        assert_eq!(v["numeric"]["count"], 2);
        assert_eq!(v["numeric"]["sum"], 30.0);
        assert_eq!(
            v["numeric"]["avg"], 15.0,
            "three blanks would drag this to 6"
        );
        assert_eq!(v["numeric"]["median"], 15.0);
    }

    /// **A number stored as text is text, and saying so is the point.**
    ///
    /// This is the single commonest thing wrong with a real column, and it is
    /// exactly what the panel exists to reveal: one `'007` in a numeric column
    /// is why the SUM is short. Coercing it here would hide the defect the user
    /// opened the panel to find.
    #[test]
    fn a_number_stored_as_text_is_text_and_stays_out_of_the_average() {
        session_new();
        set(0, "10");
        set(1, "'007"); // quote-prefixed: text, however numeric it looks
        set(2, "20");

        let v = stats(0, 0, 2, 0);
        assert_eq!(v["count"], 3);
        assert_eq!(v["types"]["number"], 2);
        assert_eq!(v["types"]["text"], 1);
        assert_eq!(
            v["types"]["numberAsText"], 1,
            "the text that looks numeric is named, not silently coerced"
        );
        assert_eq!(v["numeric"]["count"], 2);
        assert_eq!(v["numeric"]["sum"], 30.0, "coercion would make this 37");
        assert_eq!(v["numeric"]["avg"], 15.0);
    }

    /// **An error is counted, not skipped and not a number.**
    #[test]
    fn errors_are_counted_and_are_not_numbers() {
        session_new();
        set(0, "10");
        set(1, "=1/0");
        set(2, "20");

        let v = stats(0, 0, 2, 0);
        assert_eq!(v["count"], 3, "the error cell is not empty");
        assert_eq!(v["types"]["error"], 1);
        assert_eq!(v["errors"]["#DIV/0!"], 1, "broken down by token");
        assert_eq!(v["types"]["formula"], 1);
        assert_eq!(v["numeric"]["count"], 2, "an error is not a number");
        assert_eq!(v["numeric"]["avg"], 15.0);
    }

    /// Dates and booleans are their own kinds, and a date shows as a date.
    ///
    /// A date is a number underneath and the status-bar summary counts it as
    /// one; the panel agrees with the bar (`numeric.count` includes it) while
    /// still naming it separately, and the frequency row shows `2024-01-15`
    /// rather than the serial nobody can read.
    #[test]
    fn dates_and_booleans_are_split_out_of_the_type_distribution() {
        session_new();
        set(0, "2024-01-15");
        set(1, "2024-01-16");
        set(2, "=1>0");
        set(3, "5");

        let v = stats(0, 0, 3, 0);
        assert_eq!(v["types"]["date"], 2);
        assert_eq!(v["types"]["boolean"], 1);
        assert_eq!(v["types"]["number"], 1);
        assert_eq!(
            v["numeric"]["count"], 3,
            "dates are numbers to the aggregate"
        );
        let rows = v["frequency"].as_array().expect("frequency rows");
        assert!(
            rows.iter()
                .any(|e| e["value"] == "2024-01-15" && e["type"] == "date"),
            "frequency shows the date, not its serial: {rows:?}"
        );
        assert!(
            rows.iter()
                .any(|e| e["value"] == "TRUE" && e["type"] == "boolean"),
            "{rows:?}"
        );
    }

    /// **The frequency table is bounded and its order is total.**
    ///
    /// A column of 100,000 invoice numbers must not return 100,000 rows, and two
    /// identical runs must return the same rows in the same order — a panel that
    /// reshuffles equal-frequency values on every open looks broken.
    #[test]
    fn the_frequency_table_is_bounded_ordered_and_deterministic() {
        session_new();
        // v00 twelve times, v01 eleven … with v09 and v10 tied on three, which
        // is what pins the tie-break: by value, so v09 is listed and v10 is not.
        let counts = [12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 3, 1];
        let mut row = 0u32;
        for (i, n) in counts.iter().enumerate() {
            for _ in 0..*n {
                set(row, &format!("v{i:02}"));
                row += 1;
            }
        }
        let total: u32 = counts.iter().sum();
        assert_eq!(row, total);

        let json = session_column_stats(0, 0, 0, total - 1, 0);
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["count"], u64::from(total));
        assert_eq!(v["unique"], 12);
        assert_eq!(v["uniqueExact"], true);

        let rows = v["frequency"].as_array().expect("frequency rows");
        assert_eq!(rows.len(), 10, "top N, not every distinct value");
        assert_eq!(rows[0]["value"], "v00");
        assert_eq!(rows[0]["count"], 12);
        assert_eq!(rows[0]["type"], "text");
        assert_eq!(
            rows[9]["value"], "v09",
            "ties break by value, so the panel cannot flicker: {rows:?}"
        );
        assert_eq!(
            v["frequencyOther"]["values"], 2,
            "and what is not listed is still accounted for"
        );
        assert_eq!(v["frequencyOther"]["count"], 4);

        assert_eq!(
            json,
            session_column_stats(0, 0, 0, total - 1, 0),
            "two runs over the same data must agree byte for byte"
        );
    }

    /// Median over an even count is the mean of the two middle values, and the
    /// deviations are the textbook ones.
    #[test]
    fn median_min_max_and_deviation() {
        session_new();
        for (i, n) in [9, 5, 4, 4, 2, 7, 4, 5].iter().enumerate() {
            set(i as u32, &n.to_string());
        }

        let v = stats(0, 0, 7, 0);
        assert_eq!(v["numeric"]["count"], 8);
        assert_eq!(v["numeric"]["avg"], 5.0);
        assert_eq!(v["numeric"]["median"], 4.5, "sorted: 2 4 4 4 | 5 5 7 9");
        assert_eq!(v["numeric"]["min"], 2.0);
        assert_eq!(v["numeric"]["max"], 9.0);
        assert_eq!(v["numeric"]["stdevp"], 2.0);
        let sample = v["numeric"]["stdev"].as_f64().expect("sample stdev");
        assert!(
            (sample - (32.0f64 / 7.0).sqrt()).abs() < 1e-12,
            "sample (n-1) deviation, as Excel's STDEV.S: {sample}"
        );
    }

    /// A pathological column is bounded rather than returned whole, and says so.
    ///
    /// Exercised through the limits the binding passes in, so the bound is
    /// testable without building a two-million-cell fixture.
    #[test]
    fn the_scan_and_the_distinct_set_are_both_bounded() {
        session_new();
        for i in 0..20u32 {
            set(i, &format!("id{i:02}"));
        }
        let tiny = StatsLimits {
            scan: 5,
            distinct: 3,
            key_bytes: 1 << 20,
            top: 2,
        };
        let s = with_session(|w| column_stats(w.workbook(), 0, 0, 0, 19, 0, tiny)).unwrap();

        assert!(
            s.truncated,
            "the scan budget stops the pass and is reported"
        );
        assert_eq!(s.count, 5, "only the budgeted cells are counted");
        assert_eq!(s.frequency.len(), 2, "top-N rows, not one per value");
        assert!(!s.unique_exact, "20 ids cannot be tracked in three slots");
        assert_eq!(s.unique, 3);
        assert_eq!(
            s.frequency_other.count, 3,
            "one tracked-but-unlisted value plus the two never tracked"
        );
    }

    /// **The neighbouring column is not in the answer.**
    ///
    /// The scan walks the sparse store's *row band*, which spans every column;
    /// the selection is a filter applied inside it. Forget the filter and a
    /// stats panel on A silently reports B as well — a wrong answer that looks
    /// entirely plausible.
    #[test]
    fn only_the_selected_columns_are_counted() {
        session_new();
        set(0, "10");
        set(1, "20");
        for row in 0..2u32 {
            session_set_cell(0, row, 1, "999").unwrap(); // column B
        }

        let v = stats(0, 0, 1, 0);
        assert_eq!(v["cols"], 1);
        assert_eq!(v["count"], 2, "column B is not in the selection");
        assert_eq!(v["numeric"]["sum"], 30.0);
        assert_eq!(v["numeric"]["max"], 20.0);

        // Widened to A:B, the same pass now sees both.
        let both = stats(0, 0, 1, 1);
        assert_eq!(both["cols"], 2);
        assert_eq!(both["cells"], 4);
        assert_eq!(both["count"], 4);
        assert_eq!(both["numeric"]["sum"], 2028.0);
    }

    /// **A value JSON cannot spell must not become JSON that will not parse.**
    ///
    /// `format!("{}", f64::NAN)` is `NaN`, and an imported workbook can carry
    /// one. Emitted literally it throws inside the host's `JSON.parse` and takes
    /// the whole panel out over a single cell, so a non-finite aggregate is
    /// `null` — the JSON spelling of "no answer".
    #[test]
    fn a_non_finite_value_does_not_produce_unparseable_json() {
        use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

        let mut wb = Workbook::new(Id::from_parts(1, 1));
        let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
        sheet
            .cells
            .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
        sheet
            .cells
            .set(CellRef::new(1, 0), Cell::value(CellValue::Number(f64::NAN)));
        wb.sheets.push(sheet);

        let s = column_stats(&wb, 0, 0, 0, 1, 0, StatsLimits::default());
        let json = serde_json::to_string(&s).expect("serialises");
        let v: Value = serde_json::from_str(&json).expect("host can parse it");
        assert!(
            !json.contains(":NaN") && !json.contains(":inf"),
            "a bare non-finite token reached the wire: {json}"
        );
        assert_eq!(v["count"], 2, "the cell is still a value");
        assert_eq!(v["numeric"]["count"], 2);
        assert_eq!(
            v["numeric"]["sum"],
            Value::Null,
            "no answer, said in the only way JSON can say it"
        );
    }

    /// **A whole-column selection walks the data, not the address space.**
    ///
    /// `A:A` is 1,048,576 cells and three of them hold anything. Reading the
    /// rectangle cell by cell is a million lookups for an answer the sparse
    /// store already knows; the empty count is arithmetic, not a scan.
    #[test]
    fn a_whole_column_selection_walks_the_data_not_the_address_space() {
        session_new();
        set(0, "1");
        set(9, "2");
        set(999, "3");

        let started = std::time::Instant::now();
        let v = stats(0, 0, casual_calc_model::GRID_MAX_ROW, 0);
        let elapsed = started.elapsed();

        assert_eq!(v["rows"], 1_048_576);
        assert_eq!(v["count"], 3);
        assert_eq!(v["empty"], 1_048_573);
        assert_eq!(v["truncated"], false);
        assert_eq!(v["numeric"]["sum"], 6.0);
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "A:A took {elapsed:?} — that is a rectangle scan, not a data scan"
        );
    }
}
