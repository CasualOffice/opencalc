//! Conditional formatting, resolved once for every renderer.
//!
//! This lived inside `casual-calc-wasm`, which is a host crate: the browser
//! canvas got colour scales, data bars and highlight rules, and the headless
//! PNG — thumbnails, previews, server-side export — got a sheet of plain cells
//! (`RND-05`). Not because the logic was missing, but because it was in a crate
//! the render path cannot depend on.
//!
//! So it is here, where layout can reach it and the wasm bindings still can.
//! One implementation, which is the only way two renderers cannot disagree:
//! moving it also removes the possibility of fixing a rule in one of them.
//!
//! # Statistics are per range, not per cell
//!
//! A colour scale needs its range's extremes, a top-N rule its cutoff, a
//! duplicate rule how often each value occurs. Computed once per rule and
//! handed to every cell in it — a scale over a thousand rows would otherwise
//! scan the range a thousand times.

use std::collections::HashMap;

use casual_calc_model::{CellRef, CellValue, CfRule, ConditionalFormat, Sheet, Workbook};

#[derive(Debug, Clone, Default)]
pub struct RangeStats {
    /// Smallest numeric value in the range (`INFINITY` when there are none).
    min: f64,
    /// Largest numeric value.
    max: f64,
    /// Mean of the numeric values.
    mean: f64,
    /// The top-N cutoff for a `Top10` rule: a value passes when it is at least
    /// this (or at most, for `bottom`). Precomputed so the per-cell test stays
    /// a comparison rather than a re-sort.
    cutoff: f64,
    /// How many times each display value occurs, for duplicate/unique rules.
    /// Empty unless such a rule needs it — building it for every rule would
    /// allocate a string per cell for nothing.
    counts: HashMap<String, u32>,
}

impl RangeStats {
    /// Whether a cell passes a rule that needed these statistics.
    #[must_use]
    pub fn matches(&self, rule: &CfRule, value: &CellValue, text: &str) -> bool {
        match rule {
            CfRule::Top10 { bottom, .. } => {
                let CellValue::Number(n) = value else {
                    return false;
                };
                if *bottom {
                    *n <= self.cutoff
                } else {
                    *n >= self.cutoff
                }
            }
            CfRule::AboveAverage { below, equal } => {
                let CellValue::Number(n) = value else {
                    return false;
                };
                // Compare against the mean with an epsilon so a value that is
                // arithmetically equal does not fall on the wrong side of it.
                let d = n - self.mean;
                if d.abs() < 1e-9 {
                    return *equal;
                }
                if *below { d < 0.0 } else { d > 0.0 }
            }
            CfRule::DuplicateValues { unique } => {
                // A blank is neither duplicated nor unique — Excel skips them.
                if text.is_empty() {
                    return false;
                }
                let n = self.counts.get(text).copied().unwrap_or(0);
                if *unique { n == 1 } else { n > 1 }
            }
            _ => false,
        }
    }
}

#[must_use]
pub fn range_stats(wb: &Workbook, sheet: &Sheet, cf: &ConditionalFormat) -> RangeStats {
    let mut stats = RangeStats {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        ..Default::default()
    };
    if !cf.rule.needs_range_stats() {
        return stats;
    }
    let wants_counts = matches!(cf.rule, CfRule::DuplicateValues { .. });
    let mut values: Vec<f64> = Vec::new();
    for r in cf.range.start.row..=cf.range.end.row {
        for c in cf.range.start.col..=cf.range.end.col {
            let Some(cell) = sheet.cells.get(CellRef::new(r, c)) else {
                continue;
            };
            if let CellValue::Number(n) = cell.value
                && n.is_finite()
            {
                stats.min = stats.min.min(n);
                stats.max = stats.max.max(n);
                values.push(n);
            }
            if wants_counts {
                // Duplicate/unique compare what is *displayed*, so two cells
                // showing "1.0" and "1" count as different — as they do in Excel.
                // Compare what is *displayed*: two cells showing the same thing
                // are duplicates even if one is text and one a number, which is
                // how Excel treats them.
                let key = crate::display_text(wb, cell);
                if !key.is_empty() {
                    *stats.counts.entry(key).or_insert(0) += 1;
                }
            }
        }
    }
    if !values.is_empty() {
        stats.mean = values.iter().sum::<f64>() / values.len() as f64;
    }
    if let CfRule::Top10 {
        rank,
        bottom,
        percent,
    } = &cf.rule
        && !values.is_empty()
    {
        // Sort once and index, rather than testing each cell against the rest.
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = values.len();
        let take = if *percent {
            // Excel rounds a percentage down but always keeps at least one.
            (((*rank as f64 / 100.0) * n as f64).floor() as usize).clamp(1, n)
        } else {
            (*rank as usize).clamp(1, n)
        };
        stats.cutoff = if *bottom {
            values[take - 1]
        } else {
            values[n - take]
        };
    }
    stats
}

#[must_use]
pub fn scale_color(colors: &[String], t: f64) -> String {
    let parse = |hex: &str| -> (f64, f64, f64) {
        let v = u32::from_str_radix(hex, 16).unwrap_or(0);
        (
            f64::from((v >> 16) & 0xff),
            f64::from((v >> 8) & 0xff),
            f64::from(v & 0xff),
        )
    };
    if colors.is_empty() {
        return String::new();
    }
    let t = t.clamp(0.0, 1.0);
    // With three stops the midpoint is its own anchor, so each half interpolates
    // separately — otherwise the middle colour would never appear.
    let (a, b, local) = if colors.len() >= 3 {
        if t < 0.5 {
            (&colors[0], &colors[1], t * 2.0)
        } else {
            (&colors[1], &colors[2], (t - 0.5) * 2.0)
        }
    } else {
        (&colors[0], &colors[colors.len() - 1], t)
    };
    let (ar, ag, ab) = parse(a);
    let (br, bg, bb) = parse(b);
    let mix = |x: f64, y: f64| (x + (y - x) * local).round() as u32;
    format!("{:02X}{:02X}{:02X}", mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// What conditional formatting does to one cell.
///
/// Empty when no rule matched, which is the common case and costs nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellEffect {
    /// A fill that overrides the cell's own, as `RRGGBB`.
    pub fill: Option<String>,
    /// A font colour that overrides the cell's own.
    pub font_color: Option<String>,
    /// Whether a matching rule asked for bold.
    pub bold: bool,
    /// A data bar: how full, from zero to one, and its colour.
    pub data_bar: Option<(f64, String)>,
}

impl CellEffect {
    /// Whether anything applied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fill.is_none() && self.font_color.is_none() && !self.bold && self.data_bar.is_none()
    }
}

/// Where a [`CfRule::Expression`]'s per-cell answer comes from.
///
/// A formula rule is the one kind this crate cannot decide for itself. Deciding
/// it means evaluating a formula, which is `casual-calc-eval` — and that crate
/// depends on *this* one, so the arrow cannot be turned round. So the caller
/// that has an evaluator supplies the answers through this trait, and the one
/// implementation of conditional formatting stays the only one, which is the
/// whole point of the module (`RND-05`).
///
/// `&self` rather than `&mut self` deliberately: an implementation wants to
/// memoize, and the callers here hold `effect_for` inside a closure that is
/// called per cell. Interior mutability on the implementer's side keeps that
/// from spreading a `&mut` through every layout path.
pub trait CfExpressions {
    /// Whether the `rule`-th conditional format of the sheet — indexed as
    /// `Sheet::conditional_formats` is — is true at `(row, col)`.
    ///
    /// Only ever asked of a rule whose [`CfRule::formula`] is `Some`.
    fn matches(&self, rule: usize, row: u32, col: u32) -> bool;
}

/// The answer a caller with no formula engine gives: no expression matches.
///
/// Used by the layout entry points that take only a workbook. It is not the
/// same as "there is no rule" — the rule is in the model and will be written
/// back to the file — it is "this caller cannot see whether it applies".
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExpressions;

impl CfExpressions for NoExpressions {
    fn matches(&self, _rule: usize, _row: u32, _col: u32) -> bool {
        false
    }
}

/// The order rules are considered in: **by priority, not by document order**.
///
/// Lowest priority wins, and a rule with no priority sorts last. Returned
/// separately from [`effect_for`] so a caller resolving a whole range sorts
/// once rather than per cell.
#[must_use]
pub fn priority_order(sheet: &Sheet) -> Vec<usize> {
    let mut order: Vec<usize> = (0..sheet.conditional_formats.len()).collect();
    order.sort_by_key(|&i| {
        let p = sheet.conditional_formats[i].priority;
        (if p == 0 { u32::MAX } else { p }, i)
    });
    order
}

/// Resolve every rule covering one cell into a single effect.
///
/// `stats` is indexed by rule, as [`range_stats`] produced them; `order` is
/// [`priority_order`]. `text` is the cell's *display* text, because text rules
/// test what is shown rather than what is stored — a number formatted as a
/// date matches a rule looking for the year.
///
/// First match wins per property, and a matching rule with `stopIfTrue` ends
/// the search.
///
/// `exprs` decides the formula rules; pass [`NoExpressions`] when there is no
/// evaluator to hand.
// Eight, and each one is a distinct thing the answer depends on: the rules, the
// per-rule statistics, the order, the address, the value, the text, and the
// expression oracle. Bundling them into a struct would move the argument list
// to the call site and gain nothing — every caller already computes all of them.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn effect_for(
    sheet: &Sheet,
    stats: &[RangeStats],
    order: &[usize],
    row: u32,
    col: u32,
    value: &CellValue,
    text: &str,
    exprs: &dyn CfExpressions,
) -> CellEffect {
    let mut effect = CellEffect::default();
    for &i in order {
        let Some(cf) = sheet.conditional_formats.get(i) else {
            continue;
        };
        if !cf.covers(row, col) {
            continue;
        }
        if cf.rule.has_own_presentation() {
            let CellValue::Number(n) = *value else {
                continue;
            };
            let (lo, hi) = (stats[i].min, stats[i].max);
            // A flat range has no gradient to speak of; put everything at the
            // top rather than dividing by zero.
            let t = if hi > lo { (n - lo) / (hi - lo) } else { 1.0 };
            match &cf.rule {
                CfRule::ColorScale(colors) => {
                    effect.fill.get_or_insert_with(|| scale_color(colors, t));
                }
                CfRule::DataBar(color) => {
                    effect
                        .data_bar
                        .get_or_insert((bar_length(t), color.clone()));
                }
                _ => {}
            }
            continue;
        }
        let hit = if cf.rule.formula().is_some() {
            // A formula rule is decided by the formula, not by this cell's
            // value: `$D2>100` over `A2:H10` paints A2 because of what is in
            // D2. Asking `matches_number` here would test the wrong cell and
            // answer `false` for every one of them.
            exprs.matches(i, row, col)
        } else if cf.rule.needs_range_stats() {
            stats[i].matches(&cf.rule, value, text)
        } else {
            match value {
                CellValue::Number(n) => cf.rule.matches_number(*n),
                _ => cf.rule.matches_text(text),
            }
        };
        if !hit {
            continue;
        }
        if !cf.fill.is_empty() {
            effect.fill.get_or_insert_with(|| cf.fill.clone());
        }
        if let Some(fc) = &cf.font_color {
            effect.font_color.get_or_insert_with(|| fc.clone());
        }
        effect.bold |= cf.bold;
        if cf.stop_if_true {
            break;
        }
    }
    effect
}

/// The shortest bar a data bar draws, as a fraction of the cell.
///
/// A raw `(n - lo) / (hi - lo)` gives the range **minimum** a fraction of zero,
/// so the smallest value rendered nothing at all — indistinguishable from a
/// cell the rule does not cover, or from an empty one (`RND-09`). That is the
/// one value in the range a reader most wants to identify, and it was the one
/// with no mark.
///
/// The numbers are not guessed from what Excel appears to do: ECMA-376 gives
/// `dataBar` the attributes `minLength` and `maxLength`, defaulting to `10` and
/// `90` percent of the cell width, and the fraction is interpolated between
/// them. So the minimum draws a short bar and the maximum stops short of the
/// full width, which is also what makes a data bar readable — a full-width bar
/// has no edge to see.
const BAR_MIN_LENGTH: f64 = 0.10;
/// The longest, per the same `maxLength` default.
const BAR_MAX_LENGTH: f64 = 0.90;

/// A position in the range, as the fraction of the cell the bar fills.
fn bar_length(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    BAR_MIN_LENGTH + t * (BAR_MAX_LENGTH - BAR_MIN_LENGTH)
}
