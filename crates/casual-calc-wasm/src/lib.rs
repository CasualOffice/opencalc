//! `casual-calc-wasm` — the `wasm-bindgen` bridge for the browser demo.
//!
//! A thin transport over the host-agnostic engine: evaluate a formula, and open
//! an `.xlsx` → recalc → lay out a viewport → render a PNG. The same core runs
//! native on Tauri; this bridge exposes it to JavaScript. See
//! `docs/02-ARCHITECTURE.md` §Host targets and `docs/44-TAURI-DESKTOP-SHELL-DESIGN.md`.

use casual_calc_eval::recalculate;
use casual_calc_formula::parse;
use casual_calc_import::import_package;
use casual_calc_layout::{GridGeometry, Viewport, layout_viewport};
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use casual_calc_render::render_png;
use wasm_bindgen::prelude::*;

/// The engine version string.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Evaluate a single self-contained formula (e.g. `=1+2*3`, `=SUM(1,2,3)`,
/// `=IF(2>1,"yes","no")`) and return its result as a string.
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
    format_value(&workbook, &value)
}

/// Open an `.xlsx`, recalculate, and render a `width_px`×`height_px` viewport of
/// the first sheet to PNG bytes at `dpi`.
#[wasm_bindgen]
pub fn render_xlsx(
    bytes: &[u8],
    width_px: u32,
    height_px: u32,
    dpi: u32,
) -> Result<Vec<u8>, JsError> {
    let outcome = import_package(bytes.to_vec()).map_err(|e| JsError::new(&e.to_string()))?;
    let mut workbook = outcome.workbook;
    recalculate(&mut workbook);

    let geometry = GridGeometry::default();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: px_to_twips(width_px, dpi),
        height: px_to_twips(height_px, dpi),
    };
    let list = layout_viewport(&workbook, 0, &geometry, &viewport);
    render_png(&list, &geometry, &viewport, dpi).map_err(|e| JsError::new(&e.to_string()))
}

/// A short human summary of an opened `.xlsx` (sheet count, first sheet name and
/// cell count) — for the demo's status line.
#[wasm_bindgen]
pub fn describe_xlsx(bytes: &[u8]) -> Result<String, JsError> {
    let outcome = import_package(bytes.to_vec()).map_err(|e| JsError::new(&e.to_string()))?;
    let wb = outcome.workbook;
    let sheets = wb.sheets.len();
    let (name, cells) = wb
        .sheets
        .first()
        .map(|s| (s.name.clone(), s.cells.len()))
        .unwrap_or_default();
    Ok(format!(
        "{sheets} sheet(s); \"{name}\" has {cells} populated cell(s)"
    ))
}

fn px_to_twips(px: u32, dpi: u32) -> i64 {
    if dpi == 0 {
        return 0;
    }
    px as i64 * 1440 / dpi as i64
}

fn format_value(workbook: &Workbook, value: &CellValue) -> String {
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
