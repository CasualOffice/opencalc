//! Admission tests, including hostile inputs that must be rejected within
//! limits (`docs/21-PARSER-LIMITS.md`, `docs/15-CI-AND-RELEASE-GATES.md`).

use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::{Package, PackageError, PackageLimits};

/// Build an in-memory ZIP from `(name, bytes)` entries, deflate-compressed.
fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in entries {
        writer.start_file(*name, opts).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn admits_a_valid_package_and_reads_parts() {
    let bytes = make_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("xl/workbook.xml", b"<workbook/>"),
    ]);
    let mut pkg = Package::open(bytes, PackageLimits::default()).unwrap();

    assert_eq!(pkg.len(), 2);
    assert!(!pkg.is_empty());
    assert!(pkg.contains("xl/workbook.xml"));
    assert!(!pkg.contains("xl/missing.xml"));

    let names = pkg.entry_names();
    assert!(names.iter().any(|n| n == "xl/workbook.xml"));

    let part = pkg.read_part("xl/workbook.xml").unwrap();
    assert_eq!(part, b"<workbook/>");

    let entries = pkg.entries().unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn rejects_non_package() {
    let err = Package::open(b"not a zip at all".to_vec(), PackageLimits::default()).unwrap_err();
    assert_eq!(err, PackageError::NotAPackage);
    assert_eq!(err.code(), "OC-PKG-0005");
}

#[test]
fn rejects_oversized_input() {
    let bytes = make_zip(&[("a.xml", b"hello")]);
    let limits = PackageLimits {
        max_input_bytes: 8, // smaller than any real zip
        ..PackageLimits::default()
    };
    let err = Package::open(bytes, limits).unwrap_err();
    assert!(matches!(err, PackageError::InputTooLarge { .. }));
    assert_eq!(err.code(), "OC-PKG-0001");
}

#[test]
fn rejects_too_many_entries() {
    let bytes = make_zip(&[("a.xml", b"a"), ("b.xml", b"b"), ("c.xml", b"c")]);
    let limits = PackageLimits {
        max_entries: 2,
        ..PackageLimits::default()
    };
    let err = Package::open(bytes, limits).unwrap_err();
    assert!(matches!(
        err,
        PackageError::TooManyEntries { count: 3, limit: 2 }
    ));
    assert_eq!(err.code(), "OC-PKG-0002");
}

#[test]
fn rejects_zip_bomb_by_expansion_ratio() {
    // 1 MiB of zeros compresses to a tiny deflate stream → very high ratio.
    let payload = vec![0u8; 1 << 20];
    let bytes = make_zip(&[("bomb.bin", &payload)]);
    let limits = PackageLimits {
        max_expansion_ratio: 100,
        ..PackageLimits::default()
    };
    let err = Package::open(bytes, limits).unwrap_err();
    assert!(matches!(err, PackageError::ExpansionRatioExceeded { .. }));
    assert_eq!(err.code(), "OC-PKG-0003");
}

#[test]
fn rejects_expansion_over_total_cap() {
    let payload = vec![0u8; 1 << 20]; // 1 MiB uncompressed
    let bytes = make_zip(&[("big.bin", &payload)]);
    let limits = PackageLimits {
        max_total_uncompressed: 4096,  // 4 KiB cap
        max_expansion_ratio: u64::MAX, // isolate the total-size check
        ..PackageLimits::default()
    };
    let err = Package::open(bytes, limits).unwrap_err();
    assert!(matches!(err, PackageError::ExpansionTooLarge { .. }));
    assert_eq!(err.code(), "OC-PKG-0003");
}

#[test]
fn rejects_path_traversal() {
    let bytes = make_zip(&[("../../etc/passwd", b"secret")]);
    let err = Package::open(bytes, PackageLimits::default()).unwrap_err();
    assert!(matches!(err, PackageError::UnsafePath { .. }));
    assert_eq!(err.code(), "OC-PKG-0004");
}

#[test]
fn missing_part_is_reported() {
    let bytes = make_zip(&[("a.xml", b"a")]);
    let mut pkg = Package::open(bytes, PackageLimits::default()).unwrap();
    let err = pkg.read_part("nope.xml").unwrap_err();
    assert!(matches!(err, PackageError::PartNotFound { .. }));
    assert_eq!(err.code(), "OC-PKG-0006");
}
