//! `casual-calc-import` — SpreadsheetML semantic import into the normalized
//! model.
//!
//! Phase 1A: maps a SpreadsheetML package into a [`Workbook`] — cell values
//! (number, bool, shared/inline string, error), **formulas parsed to an AST**
//! (`casual-calc-formula`) with the cached value preserved, **number formats**
//! (from `styles.xml` `cellXfs`), **merged ranges**, **frozen panes**, and
//! **defined names**. A [`CompatibilityReport`] records anything not fully
//! mapped (e.g. an unparseable formula is `Degraded`, keeping its cached value).
//! Import is deterministic: fixed workbook id, sequential sheet ids, and
//! insertion-ordered interning.
//!
//! See `docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md` and
//! `docs/22-NORMALIZED-SCHEMA.md`.

mod a1;
mod error;
mod read;
mod report;
mod styles;
mod theme;

pub use error::ImportError;
pub use report::{CompatibilityEntry, CompatibilityReport, ModelOutcome, RetentionOutcome};
pub use theme::stock_theme_slots;

use std::collections::HashMap;

use casual_calc_formula::{Expr, parse as parse_formula, shift_references};
use casual_calc_model::{
    AutoFilter, Cell, CellComment, CellRange, CellRef, CellValue, CfRule, ConditionalFormat,
    CustomFilter, DataValidation, DefinedName, ErrorValue, FilterOp, FilterRule, Id, IdGenerator,
    Sheet, SheetId, SheetVisibility, StringId, Workbook,
};
use casual_calc_ooxml::{OoxmlLimits, SpreadsheetPackage};

use a1::{parse_a1, parse_range};
use read::{RawCell, parse_comments, parse_defined_names, parse_shared_strings, parse_worksheet};
use styles::{StyleSheet, parse_styles};
use theme::{ThemePalette, parse_theme};

const WORKBOOK_NAMESPACE: u64 = 0x574b_0000_0000_0000; // "WK"
const SHEET_NAMESPACE: u64 = 0x5348_0000_0000_0000; // "SH"
const SHARED_STRINGS_PART: &str = "xl/sharedStrings.xml";
const STYLES_PART: &str = "xl/styles.xml";
const THEME_PART: &str = "xl/theme/theme1.xml";
/// Relationship type suffix binding a worksheet to its comments part.
const COMMENTS_REL_SUFFIX: &str = "/comments";
/// Most areas honoured from one `sqref`. Each area materializes its own model
/// entry (a validation copies its whole value list), so an adversarial part
/// with a huge area list must not become unbounded allocation.
const MAX_SQREF_AREAS: usize = 1024;

/// The result of importing a package: the model plus its compatibility report.
#[derive(Debug)]
pub struct Import {
    /// The normalized workbook.
    pub workbook: Workbook,
    /// What was mapped, degraded, or omitted.
    pub report: CompatibilityReport,
}

/// Import a SpreadsheetML package into the normalized model.
pub fn import_package(bytes: Vec<u8>) -> Result<Import, ImportError> {
    let mut package = SpreadsheetPackage::open(bytes, OoxmlLimits::default())?;
    let mut report = CompatibilityReport::default();
    let mut workbook = Workbook::new(Id::from_parts(WORKBOOK_NAMESPACE, 1));

    // Shared strings → interned into the workbook, keeping index → StringId.
    let mut shared_ids: Vec<StringId> = Vec::new();
    if package.contains(SHARED_STRINGS_PART) {
        let xml = package.read_part(SHARED_STRINGS_PART)?;
        for value in parse_shared_strings(&xml)? {
            shared_ids.push(workbook.intern_string(&value));
        }
    }

    // Styles: the number-format code per cellXfs index. Pre-intern every xf in
    // order so the style-table order is canonical (cellXfs order) — this is what
    // lets the writer round-trip styles deterministically.
    // The theme palette must be read first: `styles.xml` states most colors as
    // a theme slot plus a tint, and those are exactly the colors Excel's
    // built-in cell styles use.
    let palette = if package.contains(THEME_PART) {
        let xml = package.read_part(THEME_PART)?;
        parse_theme(&xml)?
    } else {
        ThemePalette::default()
    };
    let stylesheet = if package.contains(STYLES_PART) {
        let xml = package.read_part(STYLES_PART)?;
        parse_styles(&xml, &palette)?
    } else {
        StyleSheet::default()
    };
    // Keep the theme itself, not just its resolved colours: a host offering a
    // colour picker should offer *this file's* theme, and the writer will need
    // it once theme linkage round-trips.
    workbook.theme_colors = palette.slots().to_vec();
    workbook.default_font_name = stylesheet.default_font_name.clone();
    workbook.default_font_size_hp = stylesheet.default_font_size_hp;
    workbook.cell_styles = stylesheet.cell_styles.clone();
    let xf_style_ids: Vec<Option<_>> = stylesheet
        .xf_styles
        .iter()
        .enumerate()
        .map(|(i, style)| {
            let mut style = style.clone();
            // Carry the named-style association. `xfId` 0 is Normal, which every
            // cell points at by default and which says nothing, so only a
            // non-zero link is worth keeping — otherwise every plain cell would
            // become "styled" and stop deduplicating.
            style.style_ref = stylesheet
                .xf_style_refs
                .get(i)
                .copied()
                .flatten()
                .filter(|id| *id != 0);
            if style.is_default() {
                None
            } else {
                Some(workbook.intern_style(style))
            }
        })
        .collect();

    // Own the sheet metadata so the package can be mutated (read) while looping.
    let sheet_meta: Vec<(String, String, String)> = package
        .sheets()
        .iter()
        .map(|s| (s.name.clone(), s.part.clone(), s.state.clone()))
        .collect();

    let mut sheet_ids = IdGenerator::new(SHEET_NAMESPACE);
    let mut sheet_ids_by_index: Vec<SheetId> = Vec::new();
    for (name, part, state) in sheet_meta {
        let xml = package.read_part(&part)?;
        let worksheet = parse_worksheet(&xml, &palette)?;
        let sheet_id = SheetId(sheet_ids.next_id());
        sheet_ids_by_index.push(sheet_id);
        let mut sheet = Sheet::new(sheet_id, name);
        // A hidden sheet that comes back visible exposes data its author put
        // away on purpose, so the state travels with the sheet.
        sheet.visibility = SheetVisibility::from_ooxml(&state);

        // Shared formulas: Excel's fill-down writes the expression once, on the
        // group's master cell, and leaves every follower's `<f>` empty. Without
        // expanding them a filled column imports as one formula plus a stack of
        // cached constants — the formulas are simply gone. Collect the masters
        // first (document order puts them before their followers, but a
        // pre-pass keeps that from being load-bearing).
        let mut shared_masters: HashMap<u32, (CellRef, Expr)> = HashMap::new();
        for raw in &worksheet.cells {
            let (Some(si), Some(text)) = (raw.shared_index, raw.formula.as_deref()) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            if let Some(at) = parse_a1(&raw.reference)
                && let Ok(expr) = parse_formula(text)
            {
                shared_masters.entry(si).or_insert((at, expr));
            }
        }

        for raw in worksheet.cells {
            let Some(cell_ref) = parse_a1(&raw.reference) else {
                report.record(
                    "cellRef",
                    ModelOutcome::Omitted,
                    RetentionOutcome::NotRetained,
                );
                continue;
            };
            let value = map_value(&raw, &shared_ids, &mut workbook, &mut report);
            let mut cell = Cell::value(value);
            if let Some(index) = raw.style_index
                && let Some(Some(style_id)) = xf_style_ids.get(index as usize)
            {
                cell.style = Some(*style_id);
            }
            match raw.formula.as_deref() {
                Some(text) if !text.trim().is_empty() => match parse_formula(text) {
                    Ok(expr) => {
                        cell.formula = Some(workbook.store_formula(expr));
                        report.record("f", ModelOutcome::Mapped, RetentionOutcome::NotApplicable);
                    }
                    Err(_) => {
                        // Cached value kept; the formula text did not parse.
                        report.record("f", ModelOutcome::Degraded, RetentionOutcome::NotRetained);
                    }
                },
                // An empty `<f>` is a shared-formula follower: rebuild it from
                // its master, shifted by the row/column delta. `$` anchors stay
                // put — the same copy/fill semantics the shifter gives the UI.
                Some(_) => {
                    let rebuilt = raw.shared_index.and_then(|si| shared_masters.get(&si)).map(
                        |(at, expr)| {
                            shift_references(
                                expr,
                                i64::from(cell_ref.row) - i64::from(at.row),
                                i64::from(cell_ref.col) - i64::from(at.col),
                            )
                        },
                    );
                    match rebuilt {
                        Some(expr) => {
                            cell.formula = Some(workbook.store_formula(expr));
                            report.record(
                                "f",
                                ModelOutcome::Mapped,
                                RetentionOutcome::NotApplicable,
                            );
                        }
                        None => {
                            report.record(
                                "f",
                                ModelOutcome::Degraded,
                                RetentionOutcome::NotRetained,
                            );
                        }
                    }
                }
                None => {}
            }
            if !cell.is_blank() {
                sheet.cells.set(cell_ref, cell);
            }
        }

        for reference in &worksheet.merges {
            match parse_range(reference) {
                Some(range) => sheet.merges.push(range),
                None => report.record(
                    "mergeCell",
                    ModelOutcome::Omitted,
                    RetentionOutcome::NotRetained,
                ),
            }
        }
        if let Some((frozen_rows, frozen_cols)) = worksheet.frozen {
            sheet.view.frozen_rows = frozen_rows;
            sheet.view.frozen_cols = frozen_cols;
        }
        if let Some(zoom) = worksheet.zoom {
            sheet.view.zoom = zoom;
        }
        sheet.view.hide_gridlines = worksheet.hide_gridlines;
        sheet.view.hide_headers = worksheet.hide_headers;
        sheet.columns.default = worksheet.col_default;
        sheet.columns.sizes = worksheet.col_sizes;
        sheet.rows.default = worksheet.row_default;
        sheet.rows.sizes = worksheet.row_sizes;
        sheet.hidden_rows = worksheet.hidden_rows;
        sheet.hidden_cols = worksheet.hidden_cols;
        sheet.row_outline_levels = worksheet.row_outline_levels;
        sheet.col_outline_levels = worksheet.col_outline_levels;
        sheet.collapsed_rows = worksheet.collapsed_rows;
        sheet.collapsed_cols = worksheet.collapsed_cols;
        if let Some(outline) = worksheet.outline {
            sheet.outline = outline;
        }
        sheet.tab_color = worksheet.tab_color;

        // Autofilter. The rows it hides arrive as ordinary `hidden="1"` rows —
        // OOXML has no separate marker — so they land in `hidden_rows` here and
        // the session re-derives `filter_hidden` from the rules once formatting
        // is available (display text is what a checklist matches on).
        if let Some(reference) = worksheet.auto_filter.as_deref()
            && let Some(range) = a1::parse_range(reference)
        {
            let mut filter = AutoFilter::new(range);
            for fc in worksheet.filter_columns {
                let rule = if fc.saw_filters {
                    let mut values = fc.values;
                    if fc.blank {
                        // `blank="1"` is the checklist's "(Blanks)" entry, which
                        // the model carries as the empty string.
                        values.push(String::new());
                    }
                    // An empty checklist would select nothing at all; Excel does
                    // not write one, and honouring it would blank the sheet.
                    if values.is_empty() {
                        continue;
                    }
                    FilterRule::Values(values)
                } else {
                    let mut ops = fc.custom.into_iter().map(|(op, value)| CustomFilter {
                        op: FilterOp::from_ooxml(&op),
                        value,
                    });
                    let Some(first) = ops.next() else {
                        continue; // a filterColumn with neither kind of child
                    };
                    FilterRule::Custom {
                        first,
                        second: ops.next(),
                        and: fc.custom_and,
                    }
                };
                filter.rules.insert(fc.col_id, rule);
            }
            sheet.auto_filter = Some(filter);
        }

        // Data-validation dropdown lists: only an inline quoted CSV in formula1
        // is modeled (a range-reference list is left for later).
        for (sqref, formula1) in worksheet.validations {
            let trimmed = formula1.trim();
            if !(trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"')) {
                continue;
            }
            let values: Vec<String> = trimmed[1..trimmed.len() - 1]
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            if values.is_empty() {
                continue;
            }
            // An sqref is a space-separated list of areas; taking only the first
            // silently dropped the validation from every other area it covers.
            // Bounded, because each area copies the value list: a hand-written
            // sqref with tens of thousands of areas would otherwise turn a small
            // part into millions of heap strings.
            for area in sqref.split_whitespace().take(MAX_SQREF_AREAS) {
                let range =
                    parse_range(area).or_else(|| parse_a1(area).map(|c| CellRange::new(c, c)));
                if let Some(range) = range {
                    sheet.validations.push(DataValidation {
                        range,
                        values: values.clone(),
                    });
                }
            }
            if sqref.split_whitespace().count() > MAX_SQREF_AREAS {
                report.record(
                    "dataValidation/sqref",
                    ModelOutcome::Degraded,
                    RetentionOutcome::NotRetained,
                );
            }
        }

        // Conditional formatting: resolve each cfRule's fill via its dxfId, its
        // range via the sqref, and its predicate via type/operator/formulas.
        // Rules without a solid fill (the only kind modeled) are skipped.
        for raw in worksheet.conditional_formats {
            // Colour scales and data bars carry their own colours and have no
            // dxfId, so the fill lookup must not gate them out.
            let scale_or_bar = matches!(raw.kind.as_str(), "colorScale" | "dataBar");
            let fill = match raw
                .dxf_id
                .and_then(|id| stylesheet.dxf_fills.get(id).cloned().flatten())
            {
                Some(f) => f,
                None if scale_or_bar => String::new(),
                None => continue,
            };
            let num = |i: usize| {
                raw.formulas
                    .get(i)
                    .and_then(|s| s.trim().parse::<f64>().ok())
            };
            let rule = match (raw.kind.as_str(), raw.operator.as_str()) {
                ("cellIs", "greaterThan") => num(0).map(CfRule::GreaterThan),
                ("cellIs", "lessThan") => num(0).map(CfRule::LessThan),
                ("cellIs", "equal") => num(0).map(CfRule::EqualTo),
                ("cellIs", "between") => match (num(0), num(1)) {
                    (Some(a), Some(b)) => Some(CfRule::Between(a, b)),
                    _ => None,
                },
                ("containsText", _) => raw.text.clone().map(CfRule::TextContains),
                ("colorScale", _) if raw.colors.len() >= 2 => {
                    Some(CfRule::ColorScale(raw.colors.clone()))
                }
                ("dataBar", _) => raw.colors.first().cloned().map(CfRule::DataBar),
                ("top10", _) => Some(CfRule::Top10 {
                    // A rank of zero would select nothing; Excel's minimum is 1.
                    rank: raw.rank.max(1),
                    bottom: raw.bottom,
                    percent: raw.percent,
                }),
                ("aboveAverage", _) => Some(CfRule::AboveAverage {
                    below: !raw.above_average,
                    equal: raw.equal_average,
                }),
                ("duplicateValues", _) => Some(CfRule::DuplicateValues { unique: false }),
                ("uniqueValues", _) => Some(CfRule::DuplicateValues { unique: true }),
                _ => None,
            };
            if let Some(rule) = rule {
                // One rule per area of the sqref — a cfRule covering "A1:A9 C1:C9"
                // used to apply to the first area only. Bounded as above.
                for area in raw.sqref.split_whitespace().take(MAX_SQREF_AREAS) {
                    let Some(range) =
                        parse_range(area).or_else(|| parse_a1(area).map(|c| CellRange::new(c, c)))
                    else {
                        continue;
                    };
                    sheet.conditional_formats.push(ConditionalFormat {
                        range,
                        rule: rule.clone(),
                        fill: fill.clone(),
                        font_color: None,
                        bold: false,
                        priority: raw.priority,
                        stop_if_true: raw.stop_if_true,
                    });
                }
            }
        }

        // Cell comments: follow the sheet's own relationships. Guessing
        // `xl/comments{index+1}.xml` only agrees with files this writer
        // produced — in anyone else's package the numbering follows which
        // sheets *have* comments, so sheet 2's notes landed on sheet 1 (or on
        // no sheet at all).
        // (A comments part is only reachable through a relationship, so a sheet
        // without one simply has no notes — there is nothing to fall back to.)
        let comments_part = package
            .related_part(&part, COMMENTS_REL_SUFFIX, &OoxmlLimits::default())?
            .filter(|p| package.contains(p));
        if let Some(comments_part) = comments_part {
            let cxml = package.read_part(&comments_part)?;
            for (reference, author, text) in parse_comments(&cxml)? {
                if !text.is_empty()
                    && let Some(at) = parse_a1(&reference)
                {
                    sheet.comments.push(CellComment { at, text, author });
                }
            }
        }

        workbook.sheets.push(sheet);
    }

    // Defined names, resolved against the sheet ids assigned above.
    let workbook_part = package.workbook_part().to_owned();
    let workbook_xml = package.read_part(&workbook_part)?;
    for (name, local_sheet, refers_to) in parse_defined_names(&workbook_xml)? {
        match parse_formula(&refers_to) {
            Ok(formula) => {
                let sheet = local_sheet.and_then(|i| sheet_ids_by_index.get(i as usize).copied());
                workbook.defined_names.push(DefinedName {
                    name,
                    sheet,
                    formula,
                });
                report.record(
                    "definedName",
                    ModelOutcome::Mapped,
                    RetentionOutcome::NotApplicable,
                );
            }
            Err(_) => report.record(
                "definedName",
                ModelOutcome::Degraded,
                RetentionOutcome::NotRetained,
            ),
        }
    }

    workbook.validate()?;
    Ok(Import { workbook, report })
}

fn map_value(
    raw: &RawCell,
    shared: &[StringId],
    workbook: &mut Workbook,
    report: &mut CompatibilityReport,
) -> CellValue {
    match raw.cell_type.as_deref() {
        None | Some("n") => raw
            .value
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .map(CellValue::Number)
            .unwrap_or(CellValue::Empty),
        Some("b") => CellValue::Bool(raw.value.as_deref() == Some("1")),
        Some("s") => match raw
            .value
            .as_deref()
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| shared.get(i).copied())
        {
            Some(id) => CellValue::SharedString(id),
            None => {
                report.record("s", ModelOutcome::Omitted, RetentionOutcome::NotRetained);
                CellValue::Empty
            }
        },
        Some("str") => raw
            .value
            .as_deref()
            .map(|s| CellValue::InlineString(workbook.intern_string(s)))
            .unwrap_or(CellValue::Empty),
        Some("inlineStr") => raw
            .inline
            .as_deref()
            .map(|s| CellValue::InlineString(workbook.intern_string(s)))
            .unwrap_or(CellValue::Empty),
        Some("e") => match raw.value.as_deref().and_then(parse_error) {
            Some(error) => CellValue::Error(error),
            None => {
                report.record("e", ModelOutcome::Omitted, RetentionOutcome::NotRetained);
                CellValue::Empty
            }
        },
        Some(other) => {
            report.record(other, ModelOutcome::Omitted, RetentionOutcome::NotRetained);
            CellValue::Empty
        }
    }
}

fn parse_error(token: &str) -> Option<ErrorValue> {
    Some(match token {
        "#REF!" => ErrorValue::Ref,
        "#VALUE!" => ErrorValue::Value,
        "#DIV/0!" => ErrorValue::Div0,
        "#N/A" => ErrorValue::Na,
        "#NAME?" => ErrorValue::Name,
        "#NULL!" => ErrorValue::Null,
        "#NUM!" => ErrorValue::Num,
        "#SPILL!" => ErrorValue::Spill,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
