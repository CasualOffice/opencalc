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
//! PNG cannot resolve a range differently — `session_chart_items` calls this rather
//! than owning a second copy.
//!
//! Like the rest of layout, this reads the model's **cached** cell values and
//! never invokes the calc engine.
//!
//! # The bound, and why it is here rather than in the plot (`CHT-11`)
//!
//! This module is the one place every chart range is read: `push_bars`,
//! `push_line`, `push_pie` and the host's own `session_chart_items` all arrive
//! here.
//! It is therefore where a range's *size* has to be capped, and until
//! [`MAX_SERIES_POINTS`] it was not capped at all. A series reference is a
//! string out of an untrusted `.xlsx`, and `$A$1:$XFD$1048576` names
//! 17,179,869,184 cells: the old `ref_cells` built one `(usize, CellRef)` per
//! cell, so **resolving that chart asked the allocator for about 206 GB and the
//! process was killed**. Measured, not reasoned about. AGENTS.md ranks a
//! resource bound third, above fidelity and performance both, and this is one.
//!
//! # How a strip is read (`CHT-10`)
//!
//! One [`CellStore::row_band`](casual_calc_model::CellStore::row_band)
//! traversal per reference, not one `get` per cell. The store is a `BTreeMap`
//! keyed row-major, so a point lookup is a fresh descent and reading ten
//! thousand of them is ten thousand descents; `row_band` is the ordered range
//! scan the same map already offers, and the model's own comment on it says it
//! exists so a caller can pay for the band rather than the sheet. Measured
//! native at 10,000 rows x 6 series: **3,091 us of point lookups becomes 455
//! us**, with the same values out.

use casual_calc_formula::stored::ABSOLUTE;
use casual_calc_formula::{Expr, parse};
use casual_calc_model::{Cell, CellRef, CellValue, Workbook};

use crate::display_text;

/// The most points one chart series may be read from, whatever it names.
///
/// **A security bound, not a picture decision.** The plot's own caps
/// ([`MAX_BAR_POLYGONS`](crate::chart::MAX_BAR_POLYGONS) and its siblings) bound
/// what is *drawn*; this bounds what is *read*, which is the allocation an
/// untrusted file controls. Without it `$A$1:$XFD$1048576` is a 206 GB request.
///
/// The number is chosen to sit far above any chart and far below the grid. A
/// 400-pixel plot has 400 distinguishable positions, so 65,536 is 160 times
/// more resolution than a screen can carry; and
/// [30](../../../docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md) budgets a million
/// cells for a whole workbook, of which one series of 65,536 is already 6.5%.
/// A chart that reaches it is not a chart being drawn small, it is a reference
/// that names a region nobody plotted on purpose.
///
/// Points past it are **dropped, and said so** —
/// [`ResolvedSeries::truncated`](crate::chart::ResolvedSeries::truncated)
/// carries the count and the legend marks the series, on the same rule
/// `CHT-08` applied to a broken reference. Silently drawing the first 65,536
/// of a million would be the picture lying about its own data, which is the
/// thing this file exists to avoid.
pub const MAX_SERIES_POINTS: usize = 65_536;

/// A rectangular strip of cells a chart reference names, after bounding.
///
/// Whole **rows** of the strip are kept or dropped, never part of one: the
/// points run in reading order across the strip's width, so cutting mid-row
/// would leave a final row that is short and an index arithmetic that has to
/// know about it. A strip is at most [`MAX_SERIES_POINTS`] points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefStrip {
    /// Index of the sheet the strip is on.
    pub sheet: usize,
    /// Top-left corner of the kept part.
    pub start: CellRef,
    /// Bottom-right corner of the kept part.
    pub end: CellRef,
    /// Points the bound refused to read. Zero for every real chart.
    pub truncated: usize,
}

impl RefStrip {
    /// How many cells the strip covers, which is how many points it yields.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows() * self.width()
    }

    /// Whether the strip covers no cells at all. Never true for a resolved
    /// strip — a strip that named nothing is `None` rather than empty — but
    /// required beside [`len`](Self::len).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many points the reference named before the bound was applied.
    #[must_use]
    pub fn named(&self) -> usize {
        self.len().saturating_add(self.truncated)
    }

    fn rows(&self) -> usize {
        (self.end.row - self.start.row) as usize + 1
    }

    fn width(&self) -> usize {
        (self.end.col - self.start.col) as usize + 1
    }
}

/// Resolve a chart's `Sheet1!$A$2:$A$9` to the strip it names, bounded.
///
/// `default_sheet` is the sheet the chart sits on, used when the reference
/// names no sheet of its own. Anything that is not a reference or a range —
/// a literal, an expression, an unparseable string, a sheet name that is not
/// in this workbook — resolves to `None` rather than to a guess.
#[must_use]
pub fn ref_strip(wb: &Workbook, default_sheet: usize, reference: &str) -> Option<RefStrip> {
    let expr = parse(reference.trim().trim_start_matches('=')).ok()?;
    // **Resolved at `ABSOLUTE`**, and not as a shrug at the origin question: a
    // chart's series reference is stored on the *chart*, not in a cell, so it
    // has no holding cell to be relative to. It is written absolutely and read
    // absolutely.
    let (a, b) = match &expr {
        Expr::Range(a, b) => (a.resolve(ABSOLUTE)?, b.resolve(ABSOLUTE)?),
        Expr::Reference(r) => {
            let one = r.resolve(ABSOLUTE)?;
            (one.clone(), one)
        }
        _ => return None,
    };
    let sheet = match &a.sheet {
        Some(name) => wb
            .sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))?,
        None => default_sheet,
    };
    let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
    let (c0, c1) = (a.col.min(b.col), a.col.max(b.col));

    // In `u64` throughout. The product of a full-grid range is 2^34, which does
    // **not** fit a `usize` on `wasm32` — and wasm32 is where the editor runs,
    // so computing this in `usize` would wrap there and nowhere the tests run.
    let width = u64::from(c1 - c0) + 1;
    let rows = u64::from(r1 - r0) + 1;
    let cap = MAX_SERIES_POINTS as u64;
    // At least one row even when the strip is wider than the whole cap: a
    // reference naming one row of the full grid still has a first row, and
    // returning nothing for it would be the bound deciding the picture.
    let rows_kept = (cap / width).clamp(1, rows);
    let truncated = usize::try_from((rows - rows_kept).saturating_mul(width)).unwrap_or(usize::MAX);

    Some(RefStrip {
        sheet,
        start: CellRef::new(r0, c0),
        end: CellRef::new(r0 + u32::try_from(rows_kept - 1).unwrap_or(0), c1),
        truncated,
    })
}

/// Read one strip in reading order, one entry per cell, gaps included.
///
/// A single ordered traversal of the store's row band, filtered to the strip's
/// columns — see the module docs for why this is not `len()` point lookups.
fn read_strip<T: Clone>(
    wb: &Workbook,
    strip: RefStrip,
    blank: T,
    of: impl Fn(&Workbook, &Cell) -> T,
) -> Vec<T> {
    let mut out = vec![blank; strip.len()];
    let Some(sheet) = wb.sheets.get(strip.sheet) else {
        return out;
    };
    let width = strip.width();
    for (at, cell) in sheet.cells.row_band(strip.start.row, strip.end.row) {
        if at.col < strip.start.col || at.col > strip.end.col {
            continue;
        }
        let i = (at.row - strip.start.row) as usize * width + (at.col - strip.start.col) as usize;
        out[i] = of(wb, cell);
    }
    out
}

/// Resolve a chart's `Sheet1!$A$2:$A$9` to the cells it names, in order.
///
/// Bounded by [`MAX_SERIES_POINTS`]; see [`ref_strip`] for what a reference
/// that resolves to nothing gives back.
#[must_use]
pub fn ref_cells(wb: &Workbook, default_sheet: usize, reference: &str) -> Vec<(usize, CellRef)> {
    let Some(strip) = ref_strip(wb, default_sheet, reference) else {
        return Vec::new();
    };
    // A chart series is a strip, and its points are in reading order along it.
    let mut out = Vec::with_capacity(strip.len());
    for r in strip.start.row..=strip.end.row {
        for c in strip.start.col..=strip.end.col {
            out.push((strip.sheet, CellRef::new(r, c)));
        }
    }
    out
}

/// An already-resolved strip's cells as display text.
///
/// Split from [`ref_text`] so a caller that has resolved the reference once —
/// [`resolve`](crate::chart::resolve), which needs the strip's `truncated`
/// count as well as its values — does not parse the same string a second time.
#[must_use]
pub fn strip_text(wb: &Workbook, strip: RefStrip) -> Vec<String> {
    read_strip(wb, strip, String::new(), display_text)
}

/// An already-resolved strip's cells as numbers. See [`strip_text`].
#[must_use]
pub fn strip_numbers(wb: &Workbook, strip: RefStrip) -> Vec<Option<f64>> {
    read_strip(wb, strip, None, |_, cell| match cell.value {
        CellValue::Number(n) => Some(n),
        CellValue::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        _ => None,
    })
}

/// A reference's cells as display text — chart category labels.
#[must_use]
pub fn ref_text(wb: &Workbook, default_sheet: usize, reference: &str) -> Vec<String> {
    match ref_strip(wb, default_sheet, reference) {
        Some(strip) => strip_text(wb, strip),
        None => Vec::new(),
    }
}

/// A reference's cells as numbers; a non-numeric cell is a gap, not a zero.
///
/// The distinction is the whole point: a plot draws nothing at a gap and draws
/// a point on the axis at a zero, and a chart of flat zeroes looks like data.
#[must_use]
pub fn ref_numbers(wb: &Workbook, default_sheet: usize, reference: &str) -> Vec<Option<f64>> {
    match ref_strip(wb, default_sheet, reference) {
        Some(strip) => strip_numbers(wb, strip),
        None => Vec::new(),
    }
}
