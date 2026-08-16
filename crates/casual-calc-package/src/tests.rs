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

/// Rewrite every declared uncompressed size in `zip` to `claim`.
///
/// Four bytes in the central directory and four in each local header — the
/// whole of the "defence" that stood between a 1 MB file and a gigabyte of
/// memory. Patched here rather than described, because a security bound that
/// has only been reasoned about is a security bound nobody has tested.
fn lie_about_sizes(mut zip: Vec<u8>, claim: u32) -> Vec<u8> {
    let mut i = 0;
    while i + 30 <= zip.len() {
        // Central directory header: uncompressed size at offset 24.
        if zip[i..i + 4] == [b'P', b'K', 1, 2] && i + 28 <= zip.len() {
            zip[i + 24..i + 28].copy_from_slice(&claim.to_le_bytes());
            i += 4;
            continue;
        }
        // Local file header: uncompressed size at offset 22.
        if zip[i..i + 4] == [b'P', b'K', 3, 4] && i + 26 <= zip.len() {
            zip[i + 22..i + 26].copy_from_slice(&claim.to_le_bytes());
            i += 4;
            continue;
        }
        i += 1;
    }
    zip
}

/// **A part may not produce more than it declared.**
///
/// Admission adds up `entry.size()` across the archive to refuse an oversized
/// package or a suspicious expansion ratio — and every one of those numbers is
/// the attacker's. With honest headers this payload is rejected as a bomb.
/// Claiming 100 bytes instead got it admitted, and `read_part` then expanded
/// the real stream in full, because the only ceiling it applied was the
/// *whole package's*. Since unmodelled parts are retained in the model, those
/// bytes then stayed resident.
#[test]
fn a_part_that_produces_more_than_it_declared_is_refused() {
    let payload = vec![0u8; 1 << 20]; // 1 MiB of zeros, deflates to almost nothing
    let honest = make_zip(&[("[Content_Types].xml", b"<Types/>"), ("big.bin", &payload)]);

    // With honest headers the ratio check catches it, which is the behaviour
    // the patched version is escaping.
    let strict = PackageLimits {
        max_expansion_ratio: 100,
        ..PackageLimits::default()
    };
    assert!(
        matches!(
            Package::open(honest.clone(), strict),
            Err(PackageError::ExpansionRatioExceeded { .. })
        ),
        "the honest package is refused, so the lie is what buys entry"
    );

    // Now claim 100 bytes. Admission sees a tiny, unremarkable archive.
    let patched = lie_about_sizes(honest, 100);
    let mut pkg =
        Package::open(patched, strict).expect("the lie gets it past admission, as it must");

    // Mapped to its length before asserting: when this fails it fails by
    // *returning the megabyte*, and a megabyte in the assertion message buries
    // the result it is trying to report.
    let outcome = pkg.read_part("big.bin").map(|part| part.len());
    let err = outcome
        .expect_err("a part that produces ten thousand times what it declared must be refused");
    assert!(
        matches!(err, PackageError::ExpansionTooLarge { .. }),
        "got {err:?}"
    );
    assert_eq!(err.code(), "OC-PKG-0003");
}

/// **The whole-package ceiling is charged against bytes that actually exist.**
///
/// Each part passing a per-part test says nothing about all of them together,
/// and the parts are retained, so the total is what stays in memory.
#[test]
fn parts_are_charged_cumulatively_against_the_package_ceiling() {
    let chunk = vec![b'x'; 4096];
    let bytes = make_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("a.bin", &chunk),
        ("b.bin", &chunk),
        ("c.bin", &chunk),
    ]);
    // Exactly enough for the three chunks and the content types: admission
    // passes, since the declared sizes are honest here and add up to this.
    let limits = PackageLimits {
        max_total_uncompressed: 12_288 + 64,
        max_expansion_ratio: u64::MAX,
        ..PackageLimits::default()
    };
    let mut pkg = Package::open(bytes, limits).expect("admitted");
    assert_eq!(pkg.read_part("a.bin").unwrap().len(), 4096);
    assert_eq!(pkg.read_part("b.bin").unwrap().len(), 4096);
    assert_eq!(pkg.read_part("c.bin").unwrap().len(), 4096);
    // A fourth read spends budget the package does not have left. Re-reading a
    // part is the cheapest way to show the ledger is cumulative rather than
    // per-part, and it is also a real shape: nothing stops a reader asking for
    // the same part twice.
    let err = pkg
        .read_part("a.bin")
        .map(|part| part.len())
        .expect_err("the budget is spent, and re-reading spends it again");
    assert!(
        matches!(err, PackageError::ExpansionTooLarge { .. }),
        "got {err:?}"
    );
}
