//! Inspection limits: the package ceiling plus per-part XML bounds.
//! See `docs/21-PARSER-LIMITS.md`.

use casual_calc_package::PackageLimits;

/// Bounds applied while inspecting a SpreadsheetML package.
#[derive(Debug, Clone, Copy)]
pub struct OoxmlLimits {
    /// Limits for the underlying ZIP/OPC package.
    pub package: PackageLimits,
    /// Maximum XML elements decoded per part.
    pub max_xml_elements: usize,
    /// Maximum XML nesting depth per part.
    pub max_xml_depth: usize,
    /// Spreadsheet-scale admission limits, counted across the whole workbook.
    pub spreadsheet: SpreadsheetLimits,
}

/// What one document may cost the model, **totalled over the workbook**.
///
/// The other limits here are per part, and that is the hole this closes: a
/// package may hold [`PackageLimits::max_entries`] parts, each admitted against
/// its own per-part ceiling, so the *total* was bounded only by the 4 GiB
/// uncompressed cap — and 4 GiB of `<c r="A1"><v>1</v></c>` is on the order of
/// a hundred million cells, from an upload of a few megabytes. Every individual
/// check passed; the sum was the attack (`SEC-002`).
///
/// So these count once, for the document, across every part that contributes.
///
/// # Choosing the numbers
///
/// The supported target is 1M populated cells ([30](../../../docs/30-PERFORMANCE-AND-CAPACITY-TARGETS.md)),
/// so the cap sits comfortably above it: a legitimate file larger than the
/// target should still open, and only a file no deployment could serve is
/// refused. Bounded is the point, not tight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpreadsheetLimits {
    /// Populated cells across every sheet.
    pub max_populated_cells: usize,
    /// Entries in the shared-string table.
    pub max_shared_strings: usize,
    /// Defined names in the workbook.
    pub max_defined_names: usize,
    /// Merged ranges across every sheet.
    pub max_merged_ranges: usize,
}

impl Default for SpreadsheetLimits {
    fn default() -> Self {
        Self {
            // Eight times the T1 target. At the model's per-cell budget this is
            // a few hundred megabytes: large enough that no real workbook meets
            // it, small enough that a crafted one cannot exhaust a host.
            max_populated_cells: 8_000_000,
            // One per populated cell is already pathological; the table is
            // meant to be shared.
            max_shared_strings: 2_000_000,
            // Excel's own practical ceiling is far below this.
            max_defined_names: 100_000,
            // A merge costs layout work on every paint, so this is tighter than
            // the cell cap by design.
            max_merged_ranges: 1_000_000,
        }
    }
}

impl Default for OoxmlLimits {
    fn default() -> Self {
        Self {
            package: PackageLimits::default(),
            max_xml_elements: 10_000_000,
            max_xml_depth: 256,
            spreadsheet: SpreadsheetLimits::default(),
        }
    }
}
