//! `casual-calc-benchmark` — reproducible micro-benchmarks emitting a versioned
//! JSON report. See `docs/29-PHASE-0-PLAN.md` and `docs/15-CI-AND-RELEASE-GATES.md`.
//!
//! Usage:
//!
//! ```text
//! casual-calc-benchmark [--smoke] [--env <label>]
//! ```
//!
//! `--smoke` runs few iterations (CI validates the report *shape* with `jq`, not
//! absolute timings). Output is a JSON `Report` on stdout.

use std::hint::black_box;
use std::io::{Cursor, Write};
use std::time::Instant;

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use casual_calc_package::{Package, PackageLimits};
use serde::Serialize;
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

/// The versioned report schema.
const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    environment: String,
    smoke: bool,
    cases: Vec<CaseReport>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseReport {
    id: String,
    iterations: usize,
    median_ns: u64,
    p95_ns: u64,
    /// Checksum of the operation's output — identical across iterations proves
    /// the benchmarked operation is deterministic.
    output_checksum: u64,
    /// Whether every iteration produced the same checksum.
    deterministic: bool,
    /// Allowed regression before CI fails, in basis points (500 = 5%).
    max_regression_basis_points: u32,
}

/// One benchmark: a stable id, a regression tolerance, and a timed operation
/// that returns a checksum of its output.
struct Bench {
    id: &'static str,
    max_regression_basis_points: u32,
    op: Box<dyn Fn() -> u64>,
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn percentile(sorted: &[u128], fraction: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (((sorted.len() - 1) as f64) * fraction).round() as usize;
    u64::try_from(sorted[index]).unwrap_or(u64::MAX)
}

fn run_bench(bench: &Bench, iterations: usize) -> CaseReport {
    let mut samples = Vec::with_capacity(iterations);
    let mut first_checksum = None;
    let mut deterministic = true;

    for _ in 0..iterations {
        let start = Instant::now();
        let checksum = black_box((bench.op)());
        samples.push(start.elapsed().as_nanos());
        match first_checksum {
            None => first_checksum = Some(checksum),
            Some(expected) if expected != checksum => deterministic = false,
            _ => {}
        }
    }

    samples.sort_unstable();
    CaseReport {
        id: bench.id.to_owned(),
        iterations,
        median_ns: percentile(&samples, 0.5),
        p95_ns: percentile(&samples, 0.95),
        output_checksum: first_checksum.unwrap_or(0),
        deterministic,
        max_regression_basis_points: bench.max_regression_basis_points,
    }
}

/// A workbook with `cells` populated cells down column A of one sheet.
fn build_workbook(cells: u32) -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Bench");
    for row in 0..cells {
        sheet.cells.set(
            CellRef::new(row, 0),
            Cell::value(CellValue::Number(row as f64)),
        );
    }
    workbook.sheets.push(sheet);
    workbook
}

/// A minimal valid `.xlsx`-shaped package as bytes.
fn build_small_package() -> Vec<u8> {
    let parts: [(&str, &[u8]); 3] = [
        (
            "[Content_Types].xml",
            b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>",
        ),
        (
            "_rels/.rels",
            b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
        ),
        (
            "xl/workbook.xml",
            b"<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"/>",
        ),
    ];
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        writer.start_file(name, opts).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn build_cases() -> Vec<Bench> {
    let workbook = build_workbook(10_000);
    let snapshot_case = Bench {
        id: "model-snapshot-roundtrip-10k",
        max_regression_basis_points: 500,
        op: Box::new(move || {
            let bytes = workbook.to_snapshot().unwrap();
            let reopened = Workbook::from_snapshot(&bytes).unwrap();
            fnv1a(&reopened.to_snapshot().unwrap())
        }),
    };

    let package_bytes = build_small_package();
    let package_case = Bench {
        id: "package-open-small",
        max_regression_basis_points: 500,
        op: Box::new(move || {
            let mut package =
                Package::open(package_bytes.clone(), PackageLimits::default()).unwrap();
            let part = package.read_part("xl/workbook.xml").unwrap();
            fnv1a(&part)
        }),
    };

    vec![snapshot_case, package_case]
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let smoke = args.iter().any(|a| a == "--smoke");
    let environment = arg_value(&args, "--env").unwrap_or_else(|| "unspecified".to_owned());
    let iterations = if smoke { 5 } else { 200 };

    let cases = build_cases()
        .iter()
        .map(|bench| run_bench(bench, iterations))
        .collect();

    let report = Report {
        schema_version: SCHEMA_VERSION,
        environment,
        smoke,
        cases,
    };

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
