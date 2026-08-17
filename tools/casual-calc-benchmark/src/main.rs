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
    /// How each measured operation grew when its input did. The part that is
    /// actually a gate — see [`ScalingReport`].
    scaling: Vec<ScalingReport>,
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

/// How a case grew when its input grew tenfold.
///
/// # Why a ratio rather than a duration
///
/// The obvious gate is "this must take under N milliseconds", and it is the
/// wrong one for CI. A shared runner varies by several times between runs, so an
/// absolute threshold is either loose enough to catch nothing or tight enough to
/// fail on a noisy neighbour — and a gate that cries wolf is turned off, which
/// is worse than never having had one.
///
/// What actually goes wrong is **complexity**: somebody makes a linear pass
/// quadratic, and the document that took a second takes a minute. That shows up
/// in how a case *scales*, and a ratio divides the hardware out. The same
/// measurement on a fast laptop and a slow runner gives the same answer, because
/// both halves moved together.
///
/// So this measures one operation at two sizes an order of magnitude apart and
/// asserts the larger did not cost disproportionately more. Linear work earns a
/// ratio near ten; quadratic earns a hundred. The budget sits between, far from
/// both, so noise cannot reach it and a genuine regression cannot hide under it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScalingReport {
    id: String,
    small_ns: u64,
    large_ns: u64,
    /// `large / small`, in hundredths, so the report stays integers.
    ratio_centi: u64,
    /// The largest ratio this case may show.
    budget_centi: u64,
    within_budget: bool,
}

/// Twenty-five times, for ten times the work.
///
/// Linear is ten, and the slack absorbs a runner that decided to schedule
/// something else halfway through. Quadratic is a hundred, and cannot fit under
/// it. Anything between is a case worth looking at rather than a number worth
/// tuning.
const SCALING_BUDGET_CENTI: u64 = 2_500;

/// Measure `work` at two sizes, with `setup` outside the clock.
///
/// The distinction matters more than it looks. The first version of the
/// incremental-recalculation case built its workbook *inside* the timed closure
/// and reported 10.4x — a number that says nothing about recalculation, because
/// constructing ten times the sheet is ten times the work on its own. A case
/// measuring its own fixture is a case that will always look linear.
fn measure_scaling_with_setup<T>(
    id: &str,
    iterations: u32,
    setup: impl Fn(u32) -> T,
    work: impl Fn(&mut T) -> u64,
) -> ScalingReport {
    let time = |cells: u32| {
        let mut samples: Vec<u128> = (0..iterations)
            .map(|_| {
                // Rebuilt per iteration, outside the clock: the edit mutates the
                // workbook, and measuring the second edit of the same cell would
                // measure a dirty set that is already clean.
                let mut fixture = setup(cells);
                let started = std::time::Instant::now();
                std::hint::black_box(work(&mut fixture));
                started.elapsed().as_nanos()
            })
            .collect();
        samples.sort_unstable();
        percentile(&samples, 0.5)
    };
    let small_ns = time(1_000).max(1);
    let large_ns = time(10_000);
    let ratio_centi = large_ns * 100 / small_ns;
    ScalingReport {
        id: id.to_owned(),
        small_ns,
        large_ns,
        ratio_centi,
        budget_centi: SCALING_BUDGET_CENTI,
        within_budget: ratio_centi <= SCALING_BUDGET_CENTI,
    }
}

/// Measure `work` at two input sizes an order of magnitude apart.
///
/// Ten times the input: far enough that linear and quadratic differ by an order
/// of magnitude, close enough that the small case still times above the clock's
/// own noise.
fn measure_scaling(id: &str, iterations: u32, work: impl Fn(u32) -> u64) -> ScalingReport {
    let time = |cells: u32| {
        let mut samples: Vec<u128> = (0..iterations)
            .map(|_| {
                let started = std::time::Instant::now();
                std::hint::black_box(work(cells));
                started.elapsed().as_nanos()
            })
            .collect();
        samples.sort_unstable();
        percentile(&samples, 0.5)
    };

    let small_ns = time(1_000).max(1);
    let large_ns = time(10_000);
    let ratio_centi = large_ns * 100 / small_ns;
    ScalingReport {
        id: id.to_owned(),
        small_ns,
        large_ns,
        ratio_centi,
        budget_centi: SCALING_BUDGET_CENTI,
        within_budget: ratio_centi <= SCALING_BUDGET_CENTI,
    }
}

/// Every case whose growth is gated.
///
/// Two subsystems rather than one, because a scaling gate covering a single path
/// says nothing about the others — and the cell store is the one the
/// million-cell target is actually about, where the snapshot round trip is what
/// opening and saving a document goes through.
/// A sheet with every cell populated across `rows` by `cols`.
///
/// Distinct from [`build_workbook`], which fills a single column: that shape is
/// right for measuring the *store*, and wrong for measuring a frame, where what
/// costs is the number of cells actually on screen.
fn build_dense_workbook(rows: u32, cols: u32) -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Frame");
    for row in 0..rows {
        for col in 0..cols {
            sheet.cells.set(
                CellRef::new(row, col),
                Cell::value(CellValue::Number(f64::from(row * cols + col))),
            );
        }
    }
    workbook.sheets.push(sheet);
    workbook
}

/// The frame rate docs/30 states as target T2.
const TARGET_FPS: u64 = 60;

/// One whole frame at [`TARGET_FPS`], in nanoseconds — about 16.6 ms.
const FRAME_NS: u64 = 1_000_000_000 / TARGET_FPS;

/// What T2 allows the **engine** inside that frame.
///
/// docs/30 does not stop at the frame: it gives a working budget of "≤ 8 ms
/// engine-side", because the browser needs the rest of the frame for
/// compositing, input and everything else on the main thread. This benchmark is
/// engine-side — layout and render, no browser — so that is the number it must
/// hold itself to, and gating on the whole frame would quietly spend the
/// browser's half.
///
/// Derived rather than written down, so the gate cannot drift from the target
/// without somebody changing the target. It was `16_666_667 * 4`: a ceiling
/// that permitted fifteen frames a second while its own comment said sixty.
const FRAME_BUDGET_NS: u64 = FRAME_NS / 2;

/// One rendered frame: lay out the visible window, then draw it.
///
/// # Why this one is a duration and the others are ratios
///
/// Everywhere else a ratio is the right gate, because absolute times on a
/// shared runner say more about the runner than the code. A frame budget is the
/// exception: sixty frames a second **is** an absolute number — 16.6 ms — and a
/// ratio cannot express "fast enough for a human to scroll".
///
/// So this measures a duration, against **one** frame.
///
/// It was four whole frames, to guard against flakiness on a shared runner.
/// Measurement says that guard was not buying anything: this renders in about
/// 0.13 ms, so the old ceiling sat five hundred times above the measurement and
/// the engine-side budget still sits sixty times above it. CI variance would
/// have to be two orders of magnitude before the tighter gate fired — and a
/// ceiling that permits fifteen frames a second is not a gate on sixty,
/// whatever its comment says.
///
/// `medianNs` is still the honest number, reported whether or not it passes;
/// `ratioCenti` is integer-divided against the budget and rounds to zero at
/// this speed, which is a fair description of the headroom rather than a
/// missing measurement.
///
/// The viewport is a realistic window over a sheet with far more cells than fit
/// in it, because rendering everything and rendering what is visible are
/// different amounts of work, and only one of them is what a frame costs.
fn measure_frame(iterations: u32) -> ScalingReport {
    use casual_calc_layout::{GridGeometry, Viewport, layout_viewport};

    // A *dense* sheet, not `build_workbook`, which fills column A only. A
    // viewport over that is a frame with forty cells of text in it, and it
    // renders in a fraction of a millisecond — a number that looks like a pass
    // and measures an empty screen. A real frame has content in every cell it
    // shows, and that is what has to fit in the budget.
    let workbook = build_dense_workbook(200, 40);
    let geometry = GridGeometry::for_sheet(&workbook.sheets[0]);
    // Roughly a maximised window on a laptop.
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 1_600,
        height: 900,
    };

    let mut samples: Vec<u128> = (0..iterations)
        .map(|_| {
            let started = std::time::Instant::now();
            let list = layout_viewport(&workbook, 0, &geometry, &viewport);
            let pixmap = casual_calc_render::render_pixmap(&list, &geometry, &viewport, 96);
            std::hint::black_box(pixmap.is_ok());
            started.elapsed().as_nanos()
        })
        .collect();
    samples.sort_unstable();
    let median_ns = percentile(&samples, 0.5);

    let budget_ns = FRAME_BUDGET_NS;
    ScalingReport {
        id: "render-frame-1600x900".to_owned(),
        small_ns: median_ns,
        large_ns: median_ns,
        // Expressed against the budget so the report stays one shape: 100 means
        // exactly at the ceiling, and under is passing.
        ratio_centi: median_ns * 100 / budget_ns,
        budget_centi: 100,
        within_budget: median_ns <= budget_ns,
    }
}

fn measure_all_scaling(iterations: u32) -> Vec<ScalingReport> {
    vec![
        measure_scaling("model-snapshot-roundtrip-scaling", iterations, |cells| {
            let workbook = build_workbook(cells);
            let bytes = workbook.to_snapshot().unwrap();
            fnv1a(
                &Workbook::from_snapshot(&bytes)
                    .unwrap()
                    .to_snapshot()
                    .unwrap(),
            )
        }),
        // Populating the store, which is what a million-cell document does on
        // its way in. A per-cell cost creeping from constant to linear is
        // invisible at ten thousand cells and fatal at a million.
        measure_scaling("model-cell-store-fill-scaling", iterations, |cells| {
            let workbook = build_workbook(cells);
            workbook.sheets[0].cells.iter().count() as u64
        }),
        measure_frame(iterations.max(5)),
        // One edit to one cell, in a sheet of `cells` formulas that do not
        // depend on it. The dirty set is one cell either way, so the *only*
        // thing that can grow with the sheet is the work done to discover that
        // — which is the per-pass precedent graph P2-002 records as needing to
        // become persistent.
        //
        // Measured rather than assumed. If this scales flat, the remaining work
        // is not on the critical path; if it scales with the sheet, this says by
        // how much, which is the number that decides whether the 50 ms target
        // at a million cells is reachable without it.
        measure_scaling_with_setup(
            "eval-incremental-edit-scaling",
            iterations,
            build_formula_workbook,
            |workbook| {
                let at = CellRef::new(0, 1);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(1.0)));
                casual_calc_eval::recalculate_incremental(workbook, &[(0, at)]);
                // Deliberately *not* a cell count. Counting walks every cell,
                // which is O(sheet) inside the clock — it swamped the kept-graph
                // measurement below entirely (a probe timing the count alone
                // reproduced that measurement to within noise) and inflated this
                // one. What is being measured has to cost less than the way it
                // is reported.
                cheap_witness(workbook, at)
            },
        ),
        // The same edit, against a session that has already made one.
        //
        // Kept next to the measurement above rather than replacing it, because
        // they answer different questions and both are real: the first edit
        // after a document opens still pays for the whole walk, and every edit
        // after it should not. This is the second kind, which is the kind a
        // person typing produces all but once.
        //
        // The warm-up edit is in the setup, outside the clock, and is what makes
        // this measure the graph being *used* rather than the graph being built.
        measure_scaling_with_setup(
            "eval-kept-graph-edit-scaling",
            iterations,
            |cells| {
                let mut workbook = build_formula_workbook(cells);
                let mut recalc = casual_calc_eval::Recalculator::new();
                let at = CellRef::new(0, 1);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(1.0)));
                recalc.recalculate(&mut workbook, &[(0, at)]);
                (workbook, recalc)
            },
            |(workbook, recalc)| {
                let at = CellRef::new(0, 1);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(2.0)));
                recalc.recalculate(workbook, &[(0, at)]);
                cheap_witness(workbook, at)
            },
        ),
        // Whether the linear range scan is the next thing that matters.
        //
        // A kept graph makes a cell-reference edit flat; a range edge is still
        // scanned linearly, once per cell popped off the propagation queue. If
        // this scales with the sheet, step four of docs/66 (row-band buckets) is
        // required rather than an optimisation — and if it does not, it is not,
        // and the number says which rather than the design note guessing.
        measure_scaling_with_setup(
            "eval-kept-graph-range-edit-scaling",
            iterations,
            |cells| {
                let mut workbook = build_range_workbook(cells);
                let mut recalc = casual_calc_eval::Recalculator::new();
                let at = CellRef::new(0, 0);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(1.0)));
                recalc.recalculate(&mut workbook, &[(0, at)]);
                (workbook, recalc)
            },
            |(workbook, recalc)| {
                let at = CellRef::new(0, 0);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(2.0)));
                recalc.recalculate(workbook, &[(0, at)]);
                cheap_witness(workbook, at)
            },
        ),
    ]
}

/// A sheet where every formula reads a *range*, which the kept graph stores as
/// one edge scanned linearly rather than as an edge per cell.
///
/// The other fixture has no ranges at all, so it exercises the `direct` hash
/// lookup and never the scan — and a measurement that cannot see the thing step
/// four proposes to fix is not evidence about whether step four is needed.
fn build_range_workbook(cells: u32) -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Calc");
    for row in 0..cells {
        sheet.cells.set(
            CellRef::new(row, 0),
            Cell::value(CellValue::Number(f64::from(row))),
        );
        // Each formula sums a ten-row window of column A, so the ranges overlap
        // and no single edit dirties more than a handful of them.
        let first = row + 1;
        let mut formula = Cell::value(CellValue::Number(0.0));
        formula.formula = Some(workbook.store_formula(
            casual_calc_formula::parse(&format!("SUM(A{}:A{})", first, first + 9)).expect("parses"),
        ));
        sheet.cells.set(CellRef::new(row, 2), formula);
    }
    workbook.sheets.push(sheet);
    workbook
}

/// An O(1) value depending on the edit, to keep `black_box` honest without
/// timing a walk of the sheet.
fn cheap_witness(workbook: &Workbook, at: CellRef) -> u64 {
    match workbook.sheets[0].cells.get(at).map(|c| &c.value) {
        Some(CellValue::Number(n)) => n.to_bits(),
        Some(_) => 1,
        None => 0,
    }
}

/// A sheet of independent formulas, so an edit's dirty set is one cell however
/// large the sheet is.
fn build_formula_workbook(cells: u32) -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Calc");
    for row in 0..cells {
        sheet.cells.set(
            CellRef::new(row, 0),
            Cell::value(CellValue::Number(f64::from(row))),
        );
        let mut formula = Cell::value(CellValue::Number(0.0));
        formula.formula = Some(workbook.store_formula(
            casual_calc_formula::parse(&format!("A{}*2", row + 1)).expect("parses"),
        ));
        sheet.cells.set(CellRef::new(row, 2), formula);
    }
    workbook.sheets.push(sheet);
    workbook
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
        scaling: measure_all_scaling(if smoke { 3 } else { 20 }),
    };

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

#[cfg(test)]
mod gate_tests {
    use super::{FRAME_BUDGET_NS, FRAME_NS, TARGET_FPS};

    /// **The frame gate is one frame, not several.**
    ///
    /// It was four, and the comment above it said sixty frames a second — so
    /// CI enforced fifteen while docs/30 promised sixty, and nothing said so.
    /// Measurement is what settled it: the frame renders in about 0.13 ms, so
    /// even at one frame the gate sits a hundred and twenty-five times above
    /// the measurement and the looseness bought no protection from anything.
    #[test]
    fn the_frame_budget_is_the_engine_side_working_budget() {
        assert_eq!(TARGET_FPS, 60, "docs/30 T2 states sixty frames a second");
        assert_eq!(FRAME_NS, 16_666_666, "one frame at sixty a second");
        // docs/30 T2: "working budget ≤ 8 ms engine-side". This benchmark is
        // engine-side, so gating on the whole frame would spend the browser's
        // half of it on ourselves.
        // 8.33 ms: half a frame, which is the "≤ 8 ms engine-side" docs/30
        // allows. Pinned exactly, so widening it back is an edit somebody makes
        // rather than a multiplier that drifts.
        assert_eq!(FRAME_BUDGET_NS, 8_333_333);
    }
}
