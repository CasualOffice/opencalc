//! Workbook and worksheet part discovery over an admitted package.

use casual_calc_package::Package;

use crate::error::OoxmlError;
use crate::limits::OoxmlLimits;
use std::collections::BTreeMap;

use crate::opc::{
    Relationship, attr_value, parse_relationships, parse_sheet_refs, rels_part_for, resolve_target,
};

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
    /// The raw `state` attribute, empty when the sheet is visible.
    pub state: String,
}

/// An admitted SpreadsheetML package with its workbook and sheet parts
/// resolved. Reads happen on demand through the bounded underlying [`Package`].
#[derive(Debug)]
pub struct SpreadsheetPackage {
    package: Package,
    workbook_part: String,
    sheets: Vec<SheetEntry>,
    /// The bounds this package was admitted under.
    ///
    /// Kept because they apply to every part read afterwards, not just to the
    /// zip: `max_xml_elements` and `max_xml_depth` bound each `.rels` and each
    /// worksheet. Only `limits.package` reached `Package`, so a caller who
    /// tightened the XML bounds had them silently ignored from the second part
    /// onwards — every later reader reached for the defaults instead.
    limits: OoxmlLimits,
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
                state: sheet_ref.state,
            });
        }

        Ok(Self {
            package,
            workbook_part,
            sheets,
            limits,
        })
    }

    /// The bounds this package was admitted under, for readers that go on to
    /// parse further parts from it.
    pub fn limits(&self) -> &OoxmlLimits {
        &self.limits
    }

    /// The resolved workbook part path (e.g. `xl/workbook.xml`).
    pub fn workbook_part(&self) -> &str {
        &self.workbook_part
    }

    /// The discovered worksheets, in workbook order.
    pub fn sheets(&self) -> &[SheetEntry] {
        &self.sheets
    }

    /// Whether a part with the given path exists.
    pub fn contains(&self, name: &str) -> bool {
        self.package.contains(name)
    }

    /// Read a part's bytes through the bounded package.
    pub fn read_part(&mut self, name: &str) -> Result<Vec<u8>, OoxmlError> {
        Ok(self.package.read_part(name)?)
    }

    /// The part a relationship of `rel_type` points to from `part`, resolved
    /// against `part`'s directory. `None` when `part` has no `.rels`, or no
    /// relationship of that type.
    ///
    /// Use this rather than guessing a sibling's path: a package is free to
    /// name and number its parts however it likes, and only the OPC graph says
    /// which one belongs to which sheet.
    pub fn related_part(
        &mut self,
        part: &str,
        rel_type_suffix: &str,
        limits: &OoxmlLimits,
    ) -> Result<Option<String>, OoxmlError> {
        let rels_part = rels_part_for(part);
        if !self.package.contains(&rels_part) {
            return Ok(None);
        }
        let xml = self.package.read_part(&rels_part)?;
        let target = parse_relationships(&xml, limits)?
            .into_iter()
            .find(|r| r.rel_type.ends_with(rel_type_suffix))
            .map(|r| resolve_target(part, &r.target));
        Ok(target)
    }

    /// Every relationship declared by a part, by id.
    ///
    /// [`related_part`](Self::related_part) finds the one relationship of a
    /// given type; a worksheet's hyperlinks each name their own `r:id`, so they
    /// need the whole table rather than a lookup by type.
    pub fn relationships_of(
        &mut self,
        part: &str,
        limits: &OoxmlLimits,
    ) -> Result<Vec<Relationship>, OoxmlError> {
        let rels_part = rels_part_for(part);
        if !self.package.contains(&rels_part) {
            return Ok(Vec::new());
        }
        let xml = self.package.read_part(&rels_part)?;
        parse_relationships(&xml, limits)
    }

    /// The `[Content_Types].xml` `<Override>` map, part path to content type.
    ///
    /// A retained part must be re-declared here or the package is invalid, and
    /// Excel refuses to open it rather than ignoring the undeclared part.
    pub fn content_type_overrides(&mut self) -> Result<BTreeMap<String, String>, OoxmlError> {
        let mut out = BTreeMap::new();
        if !self.package.contains("[Content_Types].xml") {
            return Ok(out);
        }
        let xml = self.package.read_part("[Content_Types].xml")?;
        let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e))
                | Ok(quick_xml::events::Event::Empty(ref e))
                    if e.local_name().as_ref() == b"Override" =>
                {
                    let name = attr_value(e, b"PartName")?;
                    let ct = attr_value(e, b"ContentType")?;
                    if let (Some(name), Some(ct)) = (name, ct) {
                        out.insert(name, ct);
                    }
                }
                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        Ok(out)
    }

    /// Every entry path in the package, in archive order.
    pub fn entry_names(&self) -> Vec<String> {
        self.package.entry_names()
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
