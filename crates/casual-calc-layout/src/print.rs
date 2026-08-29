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
//! Nothing here paginates. That is deliberate and argued at
//! `session_print_html`.

use casual_calc_model::Sheet;

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
