//! Evaluating an autofilter's rules: which rows of a sheet its filters hide.
//!
//! The rules live in the model; matching them needs the text a cell *shows*,
//! which is the layout crate's, so the evaluation lives here — below every
//! host, so that opening a file derives the filtered rows before the first
//! recalculation and not after it (`XL-FEAT-136`).

use std::collections::BTreeSet;

use casual_calc_model::{AutoFilter, CellRef, CellValue, Sheet, Workbook};

/// A cell's display text and numeric value, for filter matching.
#[must_use]
pub fn filter_operands(wb: &Workbook, sheet: &Sheet, row: u32, col: u32) -> (String, Option<f64>) {
    match sheet.cells.get(CellRef::new(row, col)) {
        Some(cell) => {
            let num = match cell.value {
                CellValue::Number(n) => Some(n),
                _ => None,
            };
            (casual_calc_layout::display_text(wb, cell), num)
        }
        None => (String::new(), None),
    }
}

/// Whether `row` passes every column rule except `skip` (used to build a
/// checklist that reflects what the *other* columns have already narrowed to).
#[must_use]
pub fn row_passes(
    wb: &Workbook,
    sheet: &Sheet,
    filter: &AutoFilter,
    row: u32,
    skip: Option<u32>,
) -> bool {
    filter.rules.iter().all(|(&off, rule)| {
        if Some(off) == skip {
            return true;
        }
        let col = filter.range.start.col.saturating_add(off);
        let (text, num) = filter_operands(wb, sheet, row, col);
        rule.matches(&text, num)
    })
}

/// Every filter on a sheet: its own, then each table's, in table order. A
/// table filters independently of the sheet it sits on.
fn filters(sheet: &Sheet) -> impl Iterator<Item = &AutoFilter> {
    sheet
        .auto_filter
        .iter()
        .chain(sheet.tables.iter().filter_map(|t| t.auto_filter.as_ref()))
}

/// The rows every filter on the sheet hides, recomputed from their rules.
#[must_use]
pub fn filter_hidden_rows(wb: &Workbook, sheet: &Sheet) -> BTreeSet<u32> {
    let mut hidden = BTreeSet::new();
    for filter in filters(sheet) {
        if !filter.is_active() {
            continue;
        }
        for row in filter.body_start()..=filter.range.end.row {
            if !row_passes(wb, sheet, filter, row, None) {
                hidden.insert(row);
            }
        }
    }
    hidden
}

/// Re-derive every sheet's filtered rows after a load.
///
/// The rows arrive marked `hidden="1"` with no way to tell which of them a
/// filter hid — OOXML records no distinction. Any row a filter would hide is
/// moved out of the hand-hidden set and into the filter's, so clearing the
/// filter later releases exactly those rows, and `SUBTOTAL(1-11, …)` — which
/// counts hand-hidden rows but never filtered ones — reads the same total
/// Excel cached. A row hidden by hand that a filter *also* excludes is
/// reattributed to the filter; Excel cannot tell those apart either.
///
/// Matched against the values the file carries, which are the ones its filter
/// was applied to. Not an edit: nothing goes on the undo stack.
pub fn reconcile_filters_after_load(wb: &mut Workbook) {
    for i in 0..wb.sheets.len() {
        let sh = &wb.sheets[i];
        if !filters(sh).any(AutoFilter::is_active) {
            continue;
        }
        let hidden = filter_hidden_rows(wb, sh);
        let sh = &mut wb.sheets[i];
        for row in &hidden {
            sh.hidden_rows.remove(row);
        }
        sh.filter_hidden = hidden;
    }
}
