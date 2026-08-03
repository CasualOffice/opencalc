//! Workbook and worksheet part discovery over an admitted package.

use casual_calc_package::Package;

use crate::error::OoxmlError;
use crate::limits::OoxmlLimits;
use crate::opc::{parse_relationships, parse_sheet_refs, rels_part_for, resolve_target};

const ROOT_RELS: &str = "_rels/.rels";
const OFFICE_DOCUMENT_SUFFIX: &str = "/officeDocument";

/// A discovered worksheet: its name, workbook-local id, and part path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetEntry {
    /// Display name (tab label).
    pub name: String,
    /// Workbook-local sheet id.
    pub sheet_id: u32,
    /// The worksheet part path within the package.
    pub part: String,
}

/// An admitted SpreadsheetML package with its workbook and sheet parts
/// resolved. Reads happen on demand through the bounded underlying [`Package`].
#[derive(Debug)]
pub struct SpreadsheetPackage {
    package: Package,
    workbook_part: String,
    sheets: Vec<SheetEntry>,
}

impl SpreadsheetPackage {
    /// Admit `bytes` and resolve the workbook part and its worksheets.
    ///
    /// Follows the OPC graph: root relationships → the `officeDocument`
    /// (workbook) part → its `<sheets>` → the workbook relationships that map
    /// each `r:id` to a worksheet part.
    pub fn open(bytes: Vec<u8>, limits: OoxmlLimits) -> Result<Self, OoxmlError> {
        let mut package = Package::open(bytes, limits.package)?;

        let root_rels = read_required(&mut package, ROOT_RELS)?;
        let relationships = parse_relationships(&root_rels, &limits)?;
        let office = relationships
            .iter()
            .find(|r| r.rel_type.ends_with(OFFICE_DOCUMENT_SUFFIX))
            .ok_or_else(|| OoxmlError::UnresolvableRelationship {
                id: OFFICE_DOCUMENT_SUFFIX.to_owned(),
            })?;
        let workbook_part = resolve_target("", &office.target);

        let workbook_xml = read_required(&mut package, &workbook_part)?;
        let sheet_refs = parse_sheet_refs(&workbook_xml, &limits)?;

        let workbook_rels_part = rels_part_for(&workbook_part);
        let workbook_rels_xml = read_required(&mut package, &workbook_rels_part)?;
        let workbook_rels = parse_relationships(&workbook_rels_xml, &limits)?;

        let mut sheets = Vec::with_capacity(sheet_refs.len());
        for sheet_ref in sheet_refs {
            let rel = workbook_rels
                .iter()
                .find(|r| r.id == sheet_ref.rel_id)
                .ok_or_else(|| OoxmlError::UnresolvableRelationship {
                    id: sheet_ref.rel_id.clone(),
                })?;
            sheets.push(SheetEntry {
                name: sheet_ref.name,
                sheet_id: sheet_ref.sheet_id,
                part: resolve_target(&workbook_part, &rel.target),
            });
        }

        Ok(Self {
            package,
            workbook_part,
            sheets,
        })
    }

    /// The resolved workbook part path (e.g. `xl/workbook.xml`).
    pub fn workbook_part(&self) -> &str {
        &self.workbook_part
    }

    /// The discovered worksheets, in workbook order.
    pub fn sheets(&self) -> &[SheetEntry] {
        &self.sheets
    }

    /// Read a part's bytes through the bounded package.
    pub fn read_part(&mut self, name: &str) -> Result<Vec<u8>, OoxmlError> {
        Ok(self.package.read_part(name)?)
    }

    /// Consume this inspector, returning the underlying package.
    pub fn into_package(self) -> Package {
        self.package
    }
}

/// Read a part that must exist, mapping a lookup failure to `MissingPart`.
fn read_required(package: &mut Package, name: &str) -> Result<Vec<u8>, OoxmlError> {
    package
        .read_part(name)
        .map_err(|_| OoxmlError::MissingPart {
            name: name.to_owned(),
        })
}
