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
}

impl Default for OoxmlLimits {
    fn default() -> Self {
        Self {
            package: PackageLimits::default(),
            max_xml_elements: 10_000_000,
            max_xml_depth: 256,
        }
    }
}
