//! `casual-calc-import` — SpreadsheetML semantic import into the normalized
//! model.
//!
//! Phase 1A: shared strings and worksheet **cell values** (number, bool,
//! shared/inline string, error) map into a [`Workbook`], and **formulas are
//! parsed to an AST** (`casual-calc-formula`) stored in the workbook arena with
//! the cell's cached value preserved. A [`CompatibilityReport`] records anything
//! not fully mapped (a formula that fails to parse is `Degraded`, keeping its
//! cached value). Import is deterministic: fixed workbook id, sequential sheet
//! ids, and insertion-ordered string interning.
//!
//! See `docs/34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md` and
//! `docs/22-NORMALIZED-SCHEMA.md`.

mod a1;
mod error;
mod read;
mod report;

pub use error::ImportError;
pub use report::{CompatibilityEntry, CompatibilityReport, ModelOutcome, RetentionOutcome};

use casual_calc_formula::parse as parse_formula;
use casual_calc_model::{
    Cell, CellValue, ErrorValue, Id, IdGenerator, Sheet, SheetId, StringId, Workbook,
};
use casual_calc_ooxml::{OoxmlLimits, SpreadsheetPackage};

use a1::parse_a1;
use read::{RawCell, parse_shared_strings, parse_worksheet};

const WORKBOOK_NAMESPACE: u64 = 0x574b_0000_0000_0000; // "WK"
const SHEET_NAMESPACE: u64 = 0x5348_0000_0000_0000; // "SH"
const SHARED_STRINGS_PART: &str = "xl/sharedStrings.xml";

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

    // Own the sheet metadata so the package can be mutated (read) while looping.
    let sheet_meta: Vec<(String, String)> = package
        .sheets()
        .iter()
        .map(|s| (s.name.clone(), s.part.clone()))
        .collect();

    let mut sheet_ids = IdGenerator::new(SHEET_NAMESPACE);
    for (name, part) in sheet_meta {
        let xml = package.read_part(&part)?;
        let raw_cells = parse_worksheet(&xml)?;
        let mut sheet = Sheet::new(SheetId(sheet_ids.next_id()), name);

        for raw in raw_cells {
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
            if let Some(text) = &raw.formula {
                match parse_formula(text) {
                    Ok(expr) => {
                        cell.formula = Some(workbook.store_formula(expr));
                        report.record("f", ModelOutcome::Mapped, RetentionOutcome::NotApplicable);
                    }
                    Err(_) => {
                        // Cached value kept; the formula text did not parse.
                        report.record("f", ModelOutcome::Degraded, RetentionOutcome::NotRetained);
                    }
                }
            }
            if !cell.is_blank() {
                sheet.cells.set(cell_ref, cell);
            }
        }

        workbook.sheets.push(sheet);
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
