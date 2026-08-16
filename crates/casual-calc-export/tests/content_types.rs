//! **Every part we write must be declared in the package we write it into.**
//!
//! OPC has no "unknown part" state: a part whose name matches neither an
//! `<Override>` nor a `<Default Extension>` in `[Content_Types].xml` is not a
//! part Excel ignores, it is a package Excel refuses — or offers to repair,
//! which silently discards whatever it could not account for. So an undeclared
//! part is data loss with a dialog in front of it.
//!
//! Lives in this crate because the assertion needs both halves: the importer
//! (a dev-dependency here) to read a real file, and this writer to save it.
//!
//! The test walks **every entry of the written package** rather than naming a
//! part, and that is the whole point of it. FID-17 was a class of bug, not an
//! instance: the importer recorded content types from `<Override>` alone, so
//! any part whose type came from a `<Default>` — `printerSettings*.bin`,
//! `image1.emf`, `image1.jpeg`, an embedded `attachment1.cbn` — went out
//! undeclared. A test naming `printerSettings1.bin` would have gone green the
//! moment that one part was special-cased, and the `.jpeg` beside it would
//! still have broken the file.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use casual_calc_export::write_workbook;

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/corpus")
}

/// Every corpus file, sorted so a failure names the same file everywhere.
fn files() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(corpus()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "xlsx"))
        .collect();
    found.sort();
    found
}

/// The two maps `[Content_Types].xml` carries, and nothing else.
#[derive(Default)]
struct ContentTypes {
    /// Part name (with its leading `/`) to content type.
    overrides: BTreeMap<String, String>,
    /// Lower-cased extension to content type.
    defaults: BTreeMap<String, String>,
}

impl ContentTypes {
    /// What this package says a part's type is, by the OPC rules: an
    /// `<Override>` for the exact part name wins, otherwise the `<Default>` for
    /// its extension, otherwise nothing — and nothing is the defect.
    ///
    /// Deliberately stricter than OPC about case, which folds part names. Both
    /// sides here were written by this writer from one string, so a declaration
    /// that no longer matches its own entry byte for byte is worth failing on.
    fn resolve(&self, part: &str) -> Option<&str> {
        if let Some(ct) = self.overrides.get(&format!("/{part}")) {
            return Some(ct.as_str());
        }
        let ext = part.rsplit_once('.')?.1.to_ascii_lowercase();
        self.defaults.get(&ext).map(String::as_str)
    }
}

/// Parse `[Content_Types].xml` by hand rather than through this project's XML
/// reader. The reader is one of the things under test; a bug shared between the
/// writer and the parser would cancel out and leave this green.
fn content_types(xml: &str) -> ContentTypes {
    let attr = |tag: &str, name: &str| -> Option<String> {
        let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
        let rest = &tag[at..];
        let end = rest.find('"')?;
        Some(rest[..end].to_owned())
    };
    let mut out = ContentTypes::default();
    for tag in xml.split('<').skip(1) {
        if let Some(rest) = tag.strip_prefix("Override") {
            if let (Some(part), Some(ct)) = (attr(rest, "PartName"), attr(rest, "ContentType")) {
                out.overrides.insert(part, ct);
            }
        } else if let Some(rest) = tag.strip_prefix("Default")
            && let (Some(ext), Some(ct)) = (attr(rest, "Extension"), attr(rest, "ContentType"))
        {
            out.defaults.insert(ext.to_ascii_lowercase(), ct);
        }
    }
    out
}

/// Every entry of a package, as `(path, bytes)`.
fn entries(package: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(package.to_vec())).expect("a package");
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("readable");
        out.push((name, bytes));
    }
    out
}

/// The content-type map of a package, plus every entry in it.
fn declared(package: &[u8]) -> (ContentTypes, Vec<String>) {
    let parts = entries(package);
    let types = parts
        .iter()
        .find(|(name, _)| name == "[Content_Types].xml")
        .map(|(_, bytes)| content_types(&String::from_utf8_lossy(bytes)))
        .unwrap_or_default();
    // `[Content_Types].xml` is package metadata, not a part: it declares the
    // others and is never declared itself.
    let names = parts
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name != "[Content_Types].xml")
        .collect();
    (types, names)
}

#[test]
fn the_corpus_is_present() {
    // Without this guard the tests below pass by having nothing to run on, and
    // the coverage disappears with the fixtures.
    assert!(
        files().len() >= 5,
        "expected the real-producer corpus in fixtures/corpus; found {:?}",
        files()
    );
}

/// **The gate.** Save a real file and account for every byte-range in the zip.
#[test]
fn every_part_of_a_saved_package_has_a_content_type() {
    let mut undeclared: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for path in files() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("the file is readable");
        // A file this importer refuses is not this test's subject; the corpus
        // deliberately contains one (`49609.xlsx` has no `_rels/.rels`).
        let Ok(import) = casual_calc_import::import_package(bytes) else {
            continue;
        };
        let written = write_workbook(&import.workbook).expect("a workbook we imported saves");
        checked += 1;

        let (types, parts) = declared(&written);
        for part in parts {
            if types.resolve(&part).is_none() {
                undeclared.push(format!("{name} → /{part}"));
            }
        }
    }

    assert!(
        checked >= 5,
        "only {checked} corpus files were saved at all"
    );
    assert!(
        undeclared.is_empty(),
        "{} part(s) written with no content type — Excel refuses or repairs a \
         package like this, and repairing it drops them:\n{}",
        undeclared.len(),
        undeclared.join("\n")
    );
}

/// The same gate on the **second** save.
///
/// A package this writer produced is a package this importer has to read, and
/// the declarations have to survive that hop as well: retention is not a
/// one-save property. Excel round-trips a file many times, and a type that
/// resolved once and then evaporated would leave a workbook that opens, saves,
/// and stops opening.
#[test]
fn saving_a_package_this_writer_produced_still_declares_every_part() {
    let mut undeclared: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for path in files() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("the file is readable");
        let Ok(first) = casual_calc_import::import_package(bytes) else {
            continue;
        };
        let once = write_workbook(&first.workbook).expect("saves");
        let second = casual_calc_import::import_package(once).expect("our own output reopens");
        let twice = write_workbook(&second.workbook).expect("saves again");
        checked += 1;

        let (types, parts) = declared(&twice);
        for part in parts {
            if types.resolve(&part).is_none() {
                undeclared.push(format!("{name} (second save) → /{part}"));
            }
        }
    }

    assert!(
        checked >= 5,
        "only {checked} corpus files survived two saves"
    );
    assert!(
        undeclared.is_empty(),
        "{} part(s) lost their content type on the second save:\n{}",
        undeclared.len(),
        undeclared.join("\n")
    );
}

/// The type must be **carried**, not guessed.
///
/// The tempting fix for the gate above is to map an extension to a plausible
/// type — `.bin` is usually printer settings, `.emf` is usually an image — and
/// it is wrong twice over: `.bin` is equally an OLE object or a pivot cache
/// record stream, and a guess makes the saved file assert something the source
/// file never said. So for every part that exists in both packages, the type we
/// write must be the type we read.
#[test]
fn a_saved_part_keeps_the_content_type_its_source_declared() {
    let mut changed: Vec<String> = Vec::new();

    for path in files() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("the file is readable");
        let Ok(import) = casual_calc_import::import_package(bytes.clone()) else {
            continue;
        };
        let (source_types, source_parts) = declared(&bytes);
        let written = write_workbook(&import.workbook).expect("saves");
        let (written_types, written_parts) = declared(&written);

        for part in &written_parts {
            if !source_parts.contains(part) {
                continue; // a part this writer added; it has no source type
            }
            let (Some(before), Some(after)) =
                (source_types.resolve(part), written_types.resolve(part))
            else {
                continue; // undeclared is the gate above's failure, not this one
            };
            if before != after {
                changed.push(format!("{name} → /{part}: {before} became {after}"));
            }
        }
    }

    assert!(
        changed.is_empty(),
        "{} part(s) came back with a content type the source file never gave \
         them:\n{}",
        changed.len(),
        changed.join("\n")
    );
}
