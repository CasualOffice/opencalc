//! Resolving a chart series' formula to the values it names.
//!
//! A [`ChartSeries`](casual_calc_model::ChartSeries) does not carry numbers: it
//! carries `Sheet1!$A$2:$A$9`, a *formula string*. Something has to turn that
//! into the points a plot is drawn from, and until `RND-11` that something was
//! `ref_cells` / `ref_text` / `ref_numbers` inside `casual-calc-wasm` — a
//! **host** crate. The browser canvas could therefore draw a chart and the
//! headless renderer could not, because the render path cannot depend on the
//! host and must not.
//!
//! That is the shape `RND-05` had, and it is fixed the same way: the logic was
//! never missing, it was in the wrong crate. It lives here now, where layout
//! can reach it and the WebAssembly bindings still can, so the canvas and the
//! PNG cannot resolve a range differently — `session_charts` calls this rather
//! than owning a second copy.
//!
//! Like the rest of layout, this reads the model's **cached** cell values and
//! never invokes the calc engine.

use casual_calc_formula::stored::ABSOLUTE;
use casual_calc_formula::{Expr, parse};
use casual_calc_model::{CellRef, CellValue, Workbook};

use crate::display_text;

/// Resolve a chart's `Sheet1!$A$2:$A$9` to the cells it names, in order.
///
/// `default_sheet` is the sheet the chart sits on, used when the reference
/// names no sheet of its own. Anything that is not a reference or a range —
/// a literal, an expression, an unparseable string, a sheet name that is not
/// in this workbook — resolves to no cells at all rather than to a guess.
#[must_use]
pub fn ref_cells(wb: &Workbook, default_sheet: usize, reference: &str) -> Vec<(usize, CellRef)> {
    /// Give up on a reference that does not resolve.
    macro_rules! some_or_empty {
        ($e:expr) => {
            match $e {
                Some(v) => v,
                None => return Vec::new(),
            }
        };
    }

    let Ok(expr) = parse(reference.trim().trim_start_matches('=')) else {
        return Vec::new();
    };
    // **Resolved at `ABSOLUTE`**, and not as a shrug at the origin question: a
    // chart's series reference is stored on the *chart*, not in a cell, so it
    // has no holding cell to be relative to. It is written absolutely and read
    // absolutely.
    let (a, b) = match &expr {
        Expr::Range(a, b) => (
            some_or_empty!(a.resolve(ABSOLUTE)),
            some_or_empty!(b.resolve(ABSOLUTE)),
        ),
        Expr::Reference(r) => {
            let one = some_or_empty!(r.resolve(ABSOLUTE));
            (one.clone(), one)
        }
        _ => return Vec::new(),
    };
    let target = match &a.sheet {
        Some(name) => match wb
            .sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
        {
            Some(i) => i,
            None => return Vec::new(),
        },
        None => default_sheet,
    };
    let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
    let (c0, c1) = (a.col.min(b.col), a.col.max(b.col));
    // A chart series is a strip, and its points are in reading order along it.
    let mut out = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            out.push((target, CellRef::new(r, c)));
        }
    }
    out
}

/// A reference's cells as display text — chart category labels.
#[must_use]
pub fn ref_text(wb: &Workbook, default_sheet: usize, reference: &str) -> Vec<String> {
    ref_cells(wb, default_sheet, reference)
        .into_iter()
        .map(|(si, at)| {
            wb.sheets
                .get(si)
                .and_then(|sh| sh.cells.get(at))
                .map(|cell| display_text(wb, cell))
                .unwrap_or_default()
        })
        .collect()
}

/// A reference's cells as numbers; a non-numeric cell is a gap, not a zero.
///
/// The distinction is the whole point: a plot draws nothing at a gap and draws
/// a point on the axis at a zero, and a chart of flat zeroes looks like data.
#[must_use]
pub fn ref_numbers(wb: &Workbook, default_sheet: usize, reference: &str) -> Vec<Option<f64>> {
    ref_cells(wb, default_sheet, reference)
        .into_iter()
        .map(
            |(si, at)| match wb.sheets.get(si).and_then(|sh| sh.cells.get(at)) {
                Some(cell) => match cell.value {
                    CellValue::Number(n) => Some(n),
                    CellValue::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
                    _ => None,
                },
                None => None,
            },
        )
        .collect()
}
