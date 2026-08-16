//! OPC relationship parsing and part-path resolution.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::OoxmlError;
use crate::limits::OoxmlLimits;

/// One `<Relationship>` entry from a `.rels` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// The relationship id (`Id`), referenced by `r:id`.
    pub id: String,
    /// The relationship type URI (`Type`).
    pub rel_type: String,
    /// The target part, relative to the source part (`Target`).
    pub target: String,
    /// `TargetMode`: `External` when the target is a URI rather than a part in
    /// this package.
    ///
    /// Essential for hyperlinks, where it is the only thing distinguishing a
    /// web address from a path inside the zip. Resolving an external target as
    /// a part path silently mangles the URL.
    pub external: bool,
}

/// A `<sheet>` reference from `workbook.xml`, before its part is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRef {
    /// Display name.
    pub name: String,
    /// Workbook-local sheet id.
    pub sheet_id: u32,
    /// The relationship id linking to the worksheet part.
    pub rel_id: String,
    /// The raw `state` attribute (`hidden`, `veryHidden`), empty when visible.
    pub state: String,
}

/// Walk `xml`, invoking `on_element` for each start/empty element, bounded by
/// the element-count and depth limits.
fn walk_bounded(
    xml: &[u8],
    limits: &OoxmlLimits,
    mut on_element: impl FnMut(&BytesStart<'_>) -> Result<(), OoxmlError>,
) -> Result<(), OoxmlError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut elements = 0usize;
    let mut depth = 0usize;

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|err| OoxmlError::MalformedXml(err.to_string()))?;
        match event {
            Event::Start(ref e) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(OoxmlError::TooDeep {
                        limit: limits.max_xml_depth,
                    });
                }
                elements += 1;
                if elements > limits.max_xml_elements {
                    return Err(OoxmlError::TooManyElements {
                        limit: limits.max_xml_elements,
                    });
                }
                on_element(e)?;
            }
            Event::Empty(ref e) => {
                elements += 1;
                if elements > limits.max_xml_elements {
                    return Err(OoxmlError::TooManyElements {
                        limit: limits.max_xml_elements,
                    });
                }
                on_element(e)?;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

/// Read an attribute value as an owned `String`, matched by local name.
pub(crate) fn attr_value(e: &BytesStart<'_>, local: &[u8]) -> Result<Option<String>, OoxmlError> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| OoxmlError::MalformedXml(err.to_string()))?;
        if attr.key.local_name().as_ref() == local {
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|err| OoxmlError::MalformedXml(err.to_string()))?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

/// Parse a `.rels` part into its relationships.
pub fn parse_relationships(
    xml: &[u8],
    limits: &OoxmlLimits,
) -> Result<Vec<Relationship>, OoxmlError> {
    let mut rels = Vec::new();
    walk_bounded(xml, limits, |e| {
        if e.local_name().as_ref() == b"Relationship" {
            let id = attr_value(e, b"Id")?.unwrap_or_default();
            let rel_type = attr_value(e, b"Type")?.unwrap_or_default();
            let target = attr_value(e, b"Target")?.unwrap_or_default();
            let external =
                attr_value(e, b"TargetMode")?.is_some_and(|m| m.eq_ignore_ascii_case("External"));
            rels.push(Relationship {
                id,
                rel_type,
                target,
                external,
            });
        }
        Ok(())
    })?;
    Ok(rels)
}

/// Parse the `<sheet>` references from `workbook.xml`.
pub fn parse_sheet_refs(xml: &[u8], limits: &OoxmlLimits) -> Result<Vec<SheetRef>, OoxmlError> {
    let mut sheets = Vec::new();
    walk_bounded(xml, limits, |e| {
        if e.local_name().as_ref() == b"sheet" {
            let name = attr_value(e, b"name")?.unwrap_or_default();
            let sheet_id = attr_value(e, b"sheetId")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let rel_id = attr_value(e, b"id")?.unwrap_or_default();
            let state = attr_value(e, b"state")?.unwrap_or_default();
            sheets.push(SheetRef {
                name,
                sheet_id,
                rel_id,
                state,
            });
        }
        Ok(())
    })?;
    Ok(sheets)
}

/// The directory portion of a part path (`""` if the part is at the root).
fn base_dir(part: &str) -> &str {
    match part.rfind('/') {
        Some(i) => &part[..i],
        None => "",
    }
}

/// Normalize a package path, resolving `.` and `..` and dropping empty
/// components.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.join("/")
}

/// Resolve an OPC relationship target relative to its source part.
///
/// An absolute target (`/…`) is package-root relative; otherwise it resolves
/// against the source part's directory.
pub fn resolve_target(source_part: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return normalize(absolute);
    }
    let dir = base_dir(source_part);
    if dir.is_empty() {
        normalize(target)
    } else {
        normalize(&format!("{dir}/{target}"))
    }
}

/// The `.rels` part path that carries relationships for `part`.
pub fn rels_part_for(part: &str) -> String {
    let dir = base_dir(part);
    let file = match part.rfind('/') {
        Some(i) => &part[i + 1..],
        None => part,
    };
    if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    }
}

#[cfg(test)]
mod tests {
    use super::{rels_part_for, resolve_target};

    #[test]
    fn resolves_relative_and_absolute_targets() {
        assert_eq!(resolve_target("", "xl/workbook.xml"), "xl/workbook.xml");
        assert_eq!(
            resolve_target("xl/workbook.xml", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve_target("xl/workbook.xml", "../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(
            resolve_target("xl/workbook.xml", "/xl/styles.xml"),
            "xl/styles.xml"
        );
    }

    #[test]
    fn computes_rels_part_paths() {
        assert_eq!(rels_part_for(""), "_rels/.rels");
        assert_eq!(
            rels_part_for("xl/workbook.xml"),
            "xl/_rels/workbook.xml.rels"
        );
    }
}
