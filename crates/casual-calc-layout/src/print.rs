//! Print geometry: the paper, the printable area inside its margins, and the
//! scale factor a printout is actually drawn at.
//!
//! This is the part of printing that is **arithmetic over the grid**, and it
//! lives here rather than in a host because every consumer needs the same
//! answer: the HTML print path today, and the PDF writer `IO-03` will add.
//! Two implementations of "what percentage does fit-to-width work out to" would
//! be two answers, and the divergence would only be visible by holding one
//! printout against another.
//!
//! # Pagination (`IO-03`)
//!
//! The first half of this module — paper, printable box, fit-to-page scale — is
//! `IO-05`, and it said *"nothing here paginates"*. That sentence was true of
//! the HTML print path and is now false of this module, so it is worth being
//! exact about what changed and what did not.
//!
//! The HTML path still does not paginate: it hands a `@page` rule and one long
//! table to the browser, which breaks it. **PDF cannot borrow that** — there is
//! no engine on the other side to break anything, so the writer needs to be
//! told which rows and columns land on which sheet of paper. That is arithmetic
//! over the grid, exactly like the scale factor beside it, so it lives here
//! rather than in the writer: a paginator in `casual-calc-render` would be a
//! second opinion about the same page, and the two would only be seen to
//! disagree by holding a PDF against a printout.
//!
//! [`paginate`] is the entry point. It resolves what prints ([`scope`]:
//! `Print_Area`, the used region, `Print_Titles`), works out the printable box
//! and the scale from the pieces above, then cuts the row axis and the column
//! axis into bands that fit — honouring manual breaks, which are the one input
//! that is not arithmetic and must not be overridden by it.
//!
//! ## What it honours
//!
//! Paper size and orientation, the four margins, `Print_Area`, `Print_Titles`
//! (rows *and* columns), manual row **and column** breaks, the three scale
//! controls through [`effective_scale`], `pageOrder`, hidden rows and columns
//! (already zero-sized in [`GridGeometry`]), and merges that reach past the
//! last cell carrying a value.
//!
//! ## What it does not, and the caller must not pretend otherwise
//!
//! * **Headers and footers.** [`Pagination::header_twips`] reserves nothing;
//!   the margin boxes are the HTML path's, and `&P` has no counter here yet.
//! * **Row and column headings** (`printOptions/@headings`). The HTML path
//!   prints the `A`/`1` strips; this reserves no room for them.
//! * **Centring** (`horizontalCentered` / `verticalCentered`). Content is
//!   pinned to the top-left of the printable box.
//! * **A print area of more than one rectangle.** Excel gives each rectangle
//!   its own pages; this takes the first and says so through
//!   [`PrintScope::extra_areas`].
//! * **Row height reflow.** A row taller than the printable box is put on a
//!   page of its own and overflows it, rather than being split across two.
//!   Excel does the same.
//! * **`differentFirst` / `differentOddEven`.** Nothing reads them because
//!   nothing here draws a header.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_model::{Sheet, Workbook};

use crate::GridGeometry;

/// Twips per inch. The model measures every length in twips; paper is quoted
/// in inches and millimetres.
pub const TWIPS_PER_INCH: i64 = 1440;

/// Twips per millimetre, rounded to the nearest twip at the point of use.
const TWIPS_PER_MM: f64 = TWIPS_PER_INCH as f64 / 25.4;

/// A sheet of paper: the CSS `size` keyword for it, and its portrait extent in
/// twips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paper {
    /// The CSS `@page { size: … }` keyword, or `auto` when the code is one of
    /// the many stock sizes nothing here names.
    pub css: &'static str,
    /// Portrait width in twips.
    pub width: i64,
    /// Portrait height in twips.
    pub height: i64,
}

const fn inches(w: f64, h: f64, css: &'static str) -> Paper {
    Paper {
        css,
        width: (w * TWIPS_PER_INCH as f64) as i64,
        height: (h * TWIPS_PER_INCH as f64) as i64,
    }
}

fn millimetres(w: f64, h: f64, css: &'static str) -> Paper {
    Paper {
        css,
        width: (w * TWIPS_PER_MM).round() as i64,
        height: (h * TWIPS_PER_MM).round() as i64,
    }
}

/// The paper an OOXML `paperSize` code names.
///
/// The codes are an enum of dozens of stock sizes; only the five the page-setup
/// dialog offers are named. Anything else keeps Letter's *extent* — some number
/// is needed to work out a fit-to-page scale — while reporting `auto` as the
/// CSS keyword, so the printer chooses the sheet rather than this guessing
/// wrong about it.
#[must_use]
pub fn paper(code: &str) -> Paper {
    match code {
        "1" | "" => inches(8.5, 11.0, "letter"),
        "5" => inches(8.5, 14.0, "legal"),
        "8" => millimetres(297.0, 420.0, "A3"),
        "9" => millimetres(210.0, 297.0, "A4"),
        "11" => millimetres(148.0, 210.0, "A5"),
        _ => Paper {
            css: "auto",
            ..inches(8.5, 11.0, "auto")
        },
    }
}

/// The printable area of one page, in twips: the paper turned to its
/// orientation, less the four margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageBox {
    /// Printable width in twips. Never below one twip.
    pub width: i64,
    /// Printable height in twips. Never below one twip.
    pub height: i64,
}

impl PageBox {
    /// The printable box for a paper, an orientation, and margins in inches.
    ///
    /// Margins wider than the paper would make the printable area zero or
    /// negative and every scale computed from it either infinite or inverted,
    /// so the box is floored at one twip — an absurd printout rather than a
    /// division by zero or a scale of 400% on an empty page.
    #[must_use]
    pub fn new(paper: Paper, landscape: bool, margins_in: [f64; 4]) -> Self {
        let (pw, ph) = if landscape {
            (paper.height, paper.width)
        } else {
            (paper.width, paper.height)
        };
        let twips = |v: f64| (v.max(0.0) * TWIPS_PER_INCH as f64).round() as i64;
        let [top, right, bottom, left] = margins_in;
        Self {
            width: (pw - twips(left) - twips(right)).max(1),
            height: (ph - twips(top) - twips(bottom)).max(1),
        }
    }
}

/// What the page-setup dialog's three scale controls ask for.
///
/// Excel treats them as alternatives — `<pageSetUpPr fitToPage="1"/>` selects
/// fit-to-page and `scale` selects the percentage — and the dialog here already
/// clears one when the other is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    /// A literal percentage, as `pageSetup/@scale` gives it.
    Percent(u32),
    /// Fit the sheet into this many pages across and down. `None` on either
    /// axis means that axis is unconstrained, which is what `fitToWidth="0"`
    /// means in OOXML.
    Fit {
        /// Pages across.
        wide: Option<u32>,
        /// Pages down.
        tall: Option<u32>,
    },
}

impl Scaling {
    /// The scaling a sheet's `<pageSetup>` and `<pageSetUpPr>` describe.
    #[must_use]
    pub fn from_print(sheet: &Sheet) -> Self {
        let num = |m: &std::collections::BTreeMap<String, String>, k: &str| {
            m.get(k).and_then(|v| v.trim().parse::<u32>().ok())
        };
        let fit_to_page = matches!(
            sheet.print.setup_pr.get("fitToPage").map(String::as_str),
            Some("1") | Some("true")
        );
        if fit_to_page {
            // An absent `fitToWidth`/`fitToHeight` defaults to 1 page, not to
            // unconstrained; only an explicit 0 means "as many as it takes".
            let axis = |k: &str| match num(&sheet.print.page, k) {
                None => Some(1),
                Some(0) => None,
                Some(n) => Some(n),
            };
            Scaling::Fit {
                wide: axis("fitToWidth"),
                tall: axis("fitToHeight"),
            }
        } else {
            Scaling::Percent(num(&sheet.print.page, "scale").unwrap_or(100))
        }
    }
}

/// The smallest scale a printout is ever drawn at, as a percentage. Excel's own
/// floor, and below it the paper carries nothing legible anyway.
pub const MIN_SCALE_PERCENT: u32 = 10;
/// The largest. Excel's ceiling for the percentage control.
pub const MAX_SCALE_PERCENT: u32 = 400;

/// The scale a printout is drawn at, as a fraction (`0.7` for 70%).
///
/// `content` is the unscaled extent of everything that prints, in twips.
///
/// Fit-to-page only ever **shrinks**. Excel does not enlarge a sheet to fill
/// the paper it was told to fit into, and a user who asks for "fit to 1 page
/// wide" on a three-column sheet expects it left alone, not blown up to
/// letter-width.
#[must_use]
pub fn effective_scale(scaling: Scaling, content: (i64, i64), page: PageBox) -> f64 {
    match scaling {
        Scaling::Percent(p) => f64::from(p.clamp(MIN_SCALE_PERCENT, MAX_SCALE_PERCENT)) / 100.0,
        Scaling::Fit { wide, tall } => {
            let axis = |pages: Option<u32>, avail: i64, needed: i64| -> Option<f64> {
                let pages = f64::from(pages?.max(1));
                (needed > 0).then(|| avail as f64 * pages / needed as f64)
            };
            let candidates = [
                axis(wide, page.width, content.0),
                axis(tall, page.height, content.1),
            ];
            let fit = candidates
                .into_iter()
                .flatten()
                .fold(f64::INFINITY, f64::min);
            if fit.is_finite() {
                fit.clamp(f64::from(MIN_SCALE_PERCENT) / 100.0, 1.0)
            } else {
                1.0
            }
        }
    }
}

/// The unscaled extent, in twips, of a rectangle of the grid — the sum of the
/// widths of the columns that print and the heights of the rows that print.
///
/// Hidden lines are already zero-sized in [`GridGeometry::for_sheet`], so they
/// contribute nothing without being filtered again here.
#[must_use]
pub fn content_extent(geometry: &GridGeometry, rows: (u32, u32), cols: (u32, u32)) -> (i64, i64) {
    let width = (cols.0..=cols.1).map(|c| geometry.columns.size(c)).sum();
    let height = (rows.0..=rows.1).map(|r| geometry.rows.size(r)).sum();
    (width, height)
}

// ---------------------------------------------------------------------------
// What prints
// ---------------------------------------------------------------------------

/// The rectangle of the grid a sheet prints, and the lines it repeats on every
/// page.
///
/// `Print_Area` and `Print_Titles` are not elements of their own: they are
/// ordinary **sheet-scoped defined names** with reserved names, which is why
/// this needs the whole workbook and not just the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrintScope {
    /// First and last row that prints, inclusive.
    pub rows: (u32, u32),
    /// First and last column that prints, inclusive.
    pub cols: (u32, u32),
    /// Rows repeated at the top of every page, from `Print_Titles`.
    pub title_rows: Option<(u32, u32)>,
    /// Columns repeated at the left of every page, from `Print_Titles`.
    pub title_cols: Option<(u32, u32)>,
    /// How many further rectangles a multi-area `Print_Area` named and this
    /// dropped. Zero for every ordinary sheet.
    ///
    /// Counted rather than ignored: a print area of `A1:C10,F1:H10` prints two
    /// groups of pages in Excel and one here, and a printout missing half its
    /// columns must not look like a print area that was never set.
    pub extra_areas: u32,
}

/// The text a sheet-scoped defined name refers to, e.g. `'Sheet1'!$A$1:$D$9`.
fn sheet_name_text(workbook: &Workbook, sheet: &Sheet, name: &str) -> Option<String> {
    workbook
        .defined_names
        .iter()
        .find(|d| d.sheet == Some(sheet.id) && d.name == name)
        .map(|d| d.formula.to_string())
}

/// Drop a `'Sheet'!` prefix, leaving the bare A1 part.
fn without_sheet(reference: &str) -> &str {
    reference.rsplit('!').next().unwrap_or(reference).trim()
}

/// The inclusive row and column bounds a bare A1 range names, or `None` when it
/// is not one.
fn a1_range(text: &str) -> Option<((u32, u32), (u32, u32))> {
    let text = without_sheet(text);
    let (a, b) = text.split_once(':').unwrap_or((text, text));
    let start = casual_calc_formula::parse_a1(a.trim())?;
    let end = casual_calc_formula::parse_a1(b.trim())?;
    Some((
        (start.row.min(end.row), start.row.max(end.row)),
        (start.col.min(end.col), start.col.max(end.col)),
    ))
}

/// What a sheet prints: the print area if it has one, otherwise the used
/// region, plus whatever `Print_Titles` repeats.
///
/// `None` means the sheet has nothing to print — no cells and no merges. A
/// caller should produce no pages rather than one blank one, which is what
/// Excel does with a genuinely empty sheet.
///
/// Rows and columns the sheet hides stay **inside** the bounds: they are zero
/// sized in [`GridGeometry`], so they consume no paper, and excluding them here
/// would only make the bounds harder to reason about.
#[must_use]
pub fn scope(workbook: &Workbook, sheet_index: usize) -> Option<PrintScope> {
    let sheet = workbook.sheets.get(sheet_index)?;

    // The used region. A merge reaches past the cells that carry a value — only
    // its top-left holds one — so a merged banner over empty columns would
    // otherwise fall outside it and print as a single narrow cell.
    let mut any = false;
    let (mut last_row, mut last_col) = (0u32, 0u32);
    for (at, _) in sheet.cells.iter() {
        any = true;
        last_row = last_row.max(at.row);
        last_col = last_col.max(at.col);
    }
    for merge in &sheet.merges {
        any = true;
        last_row = last_row.max(merge.start.row.max(merge.end.row));
        last_col = last_col.max(merge.start.col.max(merge.end.col));
    }
    if !any {
        return None;
    }
    let (mut rows, mut cols) = ((0u32, last_row), (0u32, last_col));

    // `Print_Area` narrows what prints. Only the first rectangle is taken; the
    // rest are counted so the loss is named rather than invisible.
    let mut extra_areas = 0;
    if let Some(area) = sheet_name_text(workbook, sheet, "Print_Area") {
        let mut parts = area.split(',');
        if let Some(first) = parts.next()
            && let Some((r, c)) = a1_range(first)
        {
            rows = r;
            cols = c;
            extra_areas =
                u32::try_from(parts.filter(|p| !p.trim().is_empty()).count()).unwrap_or(u32::MAX);
        }
    }

    // `Print_Titles` is one or two whole-axis references: `$1:$2` for rows,
    // `$A:$B` for columns, and both when a sheet repeats a header and a stub.
    // `parse_a1` deliberately refuses a bare `A` (it is also the test for "is
    // this name a reference"), which is why the axis parser exists.
    let (mut title_rows, mut title_cols) = (None, None);
    if let Some(titles) = sheet_name_text(workbook, sheet, "Print_Titles") {
        for part in titles.split(',') {
            let bare = without_sheet(part);
            let Some((a, b)) = bare.split_once(':') else {
                continue;
            };
            let (Some(start), Some(end)) = (
                casual_calc_formula::parse_a1_axis(a.trim(), false),
                casual_calc_formula::parse_a1_axis(b.trim(), true),
            ) else {
                continue;
            };
            if start.col_implicit && end.col_implicit {
                title_rows = Some((start.row.min(end.row), start.row.max(end.row)));
            } else if start.row_implicit && end.row_implicit {
                title_cols = Some((start.col.min(end.col), start.col.max(end.col)));
            }
        }
    }

    Some(PrintScope {
        rows,
        cols,
        title_rows,
        title_cols,
        extra_areas,
    })
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// The most pages one sheet is cut into, however large it is.
///
/// A bound rather than a preference. The row axis runs to a million lines and
/// the printable box can be one twip tall (margins wider than the paper are
/// floored, not refused), so an unbounded paginator on a hostile file is a
/// memory exhaustion with a page count for a multiplier — and every page here
/// costs a display list. Reaching it sets [`Pagination::truncated`], so a host
/// says "this printed the first 4096 pages" rather than silently stopping.
pub const MAX_PAGES: usize = 4096;

/// One sheet of paper: the rows and columns of the *body*, without the repeated
/// title lines, which every page carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// First and last body row, inclusive.
    pub rows: (u32, u32),
    /// First and last body column, inclusive.
    pub cols: (u32, u32),
    /// Which band across this page is in, counting from zero.
    pub across: usize,
    /// Which band down this page is in, counting from zero.
    pub down: usize,
}

/// A sheet cut into pages, with everything the writer needs to place them.
#[derive(Debug, Clone, PartialEq)]
pub struct Pagination {
    /// What prints, resolved.
    pub scope: PrintScope,
    /// The paper, at its portrait extent.
    pub paper: Paper,
    /// Whether the paper is turned.
    pub landscape: bool,
    /// The printable area inside the margins.
    pub page_box: PageBox,
    /// Top, right, bottom, left margins in twips, as the printable box was cut
    /// from the paper.
    pub margins: [i64; 4],
    /// The scale everything is drawn at, as a fraction.
    pub scale: f64,
    /// Whether `printOptions/@gridLines` asks for gridlines. **Off unless the
    /// file says otherwise** — that is Excel's default, and a printout with
    /// gridlines nobody asked for is as wrong as one without them.
    pub gridlines: bool,
    /// Unscaled width of the repeated title columns, in twips. Zero when there
    /// are none.
    pub title_width: i64,
    /// Unscaled height of the repeated title rows, in twips.
    pub title_height: i64,
    /// The pages, in the order `pageOrder` asks for.
    pub pages: Vec<Page>,
    /// Whether [`MAX_PAGES`] cut the run short.
    pub truncated: bool,
}

impl Pagination {
    /// The twips reserved above the printable box for a header. Zero: headers
    /// are not drawn yet, and reserving room for one that is never painted
    /// would move every printout down the page for nothing.
    #[must_use]
    pub fn header_twips(&self) -> i64 {
        0
    }
}

/// Whether an attribute reads as true.
fn flag(map: &BTreeMap<String, String>, key: &str) -> bool {
    matches!(map.get(key).map(String::as_str), Some("1") | Some("true"))
}

/// The zero-based line indices a manual break starts a new page at.
///
/// `<brk id="7"/>` in `<rowBreaks>` is the break Excel writes for "page break
/// above row 8": `id` is the zero-based index of the first line on the *next*
/// page, so it is used unchanged. A break at or before the first printed line
/// is dropped, because a page that starts where the content does is not a
/// break.
fn manual_breaks(breaks: &[BTreeMap<String, String>], first: u32) -> BTreeSet<u32> {
    breaks
        .iter()
        .filter_map(|b| b.get("id"))
        .filter_map(|id| id.trim().parse::<u32>().ok())
        .filter(|&id| id > first)
        .collect()
}

/// Cut one axis into bands that fit `avail` twips, breaking early wherever
/// `breaks` says to.
///
/// **At least one line per band, always.** A row taller than the paper still
/// has to print somewhere; refusing to place it would loop forever, and
/// splitting it would need the row cut in half, which Excel does not do either.
fn bands(
    axis: &crate::Axis,
    first: u32,
    last: u32,
    avail: i64,
    breaks: &BTreeSet<u32>,
    limit: usize,
) -> (Vec<(u32, u32)>, bool) {
    let avail = avail.max(1);
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut start = first;
    loop {
        if out.len() >= limit {
            return (out, true);
        }
        let mut end = start;
        let mut used = axis.size(start);
        while end < last {
            let next = end + 1;
            if breaks.contains(&next) {
                break;
            }
            let size = axis.size(next);
            if used > 0 && used.saturating_add(size) > avail {
                break;
            }
            used = used.saturating_add(size);
            end = next;
        }
        out.push((start, end));
        if end >= last {
            return (out, false);
        }
        start = end + 1;
    }
}

/// Cut a sheet into pages.
///
/// `None` when the sheet prints nothing (see [`scope`]).
///
/// The order of operations matters and is the reason this is one function: the
/// scale is computed from the **whole** printed extent, and only then is the
/// axis cut, so "fit to one page wide" produces one page across by construction
/// rather than by a loop that hopes to converge.
#[must_use]
pub fn paginate(
    workbook: &Workbook,
    sheet_index: usize,
    geometry: &GridGeometry,
) -> Option<Pagination> {
    let sheet = workbook.sheets.get(sheet_index)?;
    let scope = scope(workbook, sheet_index)?;

    fn attr<'a>(m: &'a BTreeMap<String, String>, k: &str) -> &'a str {
        m.get(k).map(String::as_str).unwrap_or("")
    }
    let inches = |k: &str, fallback: f64| {
        sheet
            .print
            .margins
            .get(k)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(fallback)
    };

    let paper = paper(attr(&sheet.print.page, "paperSize"));
    let landscape = attr(&sheet.print.page, "orientation") == "landscape";
    // Excel's own defaults for a new sheet, in inches.
    let margins_in = [
        inches("top", 0.75),
        inches("right", 0.7),
        inches("bottom", 0.75),
        inches("left", 0.7),
    ];
    let page_box = PageBox::new(paper, landscape, margins_in);
    let margins = margins_in.map(|v| (v.max(0.0) * TWIPS_PER_INCH as f64).round() as i64);

    // The repeated lines are part of what has to fit on the paper, so they are
    // in the extent the scale is computed from — otherwise "fit to one page
    // wide" would fit the body and then push the stub columns off the edge.
    let title_height = scope
        .title_rows
        .map(|(a, b)| (a..=b).map(|r| geometry.rows.size(r)).sum::<i64>())
        .unwrap_or(0);
    let title_width = scope
        .title_cols
        .map(|(a, b)| (a..=b).map(|c| geometry.columns.size(c)).sum::<i64>())
        .unwrap_or(0);

    // Titles that sit at the top of (or left of) the print area are already
    // inside it: repeating them *and* printing them as body would show the
    // header twice on page one.
    let mut body_rows = scope.rows;
    let mut body_cols = scope.cols;
    let mut repeat_h = title_height;
    let mut repeat_w = title_width;
    if let Some((t0, t1)) = scope.title_rows
        && t0 <= body_rows.0
        && t1 >= body_rows.0
    {
        body_rows.0 = t1.saturating_add(1);
    }
    if let Some((t0, t1)) = scope.title_cols
        && t0 <= body_cols.0
        && t1 >= body_cols.0
    {
        body_cols.0 = t1.saturating_add(1);
    }
    // The print area was nothing *but* its titles. Print it once, as itself.
    if body_rows.0 > body_rows.1 {
        body_rows = scope.rows;
        repeat_h = 0;
    }
    if body_cols.0 > body_cols.1 {
        body_cols = scope.cols;
        repeat_w = 0;
    }

    let (body_w, body_h) = content_extent(geometry, body_rows, body_cols);
    let scaling = Scaling::from_print(sheet);
    let scale = effective_scale(
        scaling,
        (
            body_w.saturating_add(repeat_w),
            body_h.saturating_add(repeat_h),
        ),
        page_box,
    );

    // Available *content* twips: the printable box divided by the scale it is
    // drawn at, less what the repeated lines take on every page.
    let unscale = |v: i64| ((v as f64) / scale.max(f64::MIN_POSITIVE)).floor() as i64;
    let avail_w = unscale(page_box.width).saturating_sub(repeat_w);
    let avail_h = unscale(page_box.height).saturating_sub(repeat_h);

    let row_breaks = manual_breaks(&sheet.print.row_breaks, body_rows.0);
    let col_breaks = manual_breaks(&sheet.print.col_breaks, body_cols.0);
    let (col_bands, col_cut) = bands(
        &geometry.columns,
        body_cols.0,
        body_cols.1,
        avail_w,
        &col_breaks,
        MAX_PAGES,
    );
    let (row_bands, row_cut) = bands(
        &geometry.rows,
        body_rows.0,
        body_rows.1,
        avail_h,
        &row_breaks,
        MAX_PAGES,
    );

    // `downThenOver` is the default, and means the page *numbering* runs down
    // the rows before it moves right — so the column band is the outer loop.
    let over_then_down = attr(&sheet.print.page, "pageOrder") == "overThenDown";
    let (outers, inners) = if over_then_down {
        (row_bands.len(), col_bands.len())
    } else {
        (col_bands.len(), row_bands.len())
    };
    let mut pages = Vec::new();
    let mut truncated = col_cut || row_cut;
    'outer: for outer in 0..outers {
        for inner in 0..inners {
            let (across, down) = if over_then_down {
                (inner, outer)
            } else {
                (outer, inner)
            };
            if pages.len() >= MAX_PAGES {
                truncated = true;
                break 'outer;
            }
            pages.push(Page {
                rows: row_bands[down],
                cols: col_bands[across],
                across,
                down,
            });
        }
    }

    Some(Pagination {
        scope,
        paper,
        landscape,
        page_box,
        margins,
        scale,
        gridlines: flag(&sheet.print.options, "gridLines"),
        title_width: repeat_w,
        title_height: repeat_h,
        pages,
        truncated,
    })
}
