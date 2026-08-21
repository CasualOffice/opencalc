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
    /// `"release"` or `"debug"`.
    ///
    /// Reported because the frame budget is an absolute duration and a debug
    /// build is a different program: the same frame measures 0.13 ms in release
    /// and 5.7 ms in debug on the same machine, forty times apart. A gate on
    /// "sixty frames a second" run against a debug build is not measuring
    /// anything a user will ever experience, and CI had been doing exactly that
    /// — which is most of why the ceiling had drifted to four whole frames.
    profile: &'static str,
    smoke: bool,
    /// How each measured operation grew when its input did. The part that is
    /// actually a gate — see [`ScalingReport`].
    scaling: Vec<ScalingReport>,
    /// What a million cells cost in resident memory — see [`MemoryReport`].
    memory: MemoryReport,
    /// What a filled-down column's formula trees cost — see [`ArenaReport`].
    arena: ArenaReport,
    cases: Vec<CaseReport>,
}

/// What the formula arena costs for a filled-down column.
///
/// The number `PERF-11` turns on. `docs/75` asks whether the win is theoretical
/// — "if real workbooks turn out not to be dominated by filled-down columns" —
/// and nothing measured it, so the migration was going to be judged on an
/// intuition.
///
/// **Counted, not sampled.** Nodes times `size_of::<Expr>()` is exact and
/// deterministic, so unlike [`MemoryReport`] this works on a machine with no
/// `/proc` — which is every developer machine here. It also isolates the arena
/// from everything else in the workbook, which resident set cannot.
///
/// It understates rather than flatters: heap held *inside* a node — the
/// `Option<String>` sheet name on a reference, a `Text` literal — is not
/// counted, so the real arena is at least this big.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArenaReport {
    /// Cells in the filled-down column measured.
    cells: u64,
    /// Distinct trees the arena holds for them.
    ///
    /// **The figure `PERF-11` moves**: one shape filled down a column is one
    /// tree once references are stored relative, and `cells` of them until
    /// then. `PERF-09` already collapses *identical* trees, which a filled
    /// column does not produce — the references shift, so every row is a
    /// different tree.
    distinct_trees: u64,
    /// Total `Expr` nodes across those trees.
    nodes: u64,
    /// `nodes * size_of::<Expr>()`.
    arena_bytes: u64,
    /// What one `Expr` occupies, so the arithmetic above can be checked.
    expr_bytes: u64,
    /// Arena bytes per formula cell, in hundredths so the report stays integers.
    ///
    /// Compare against `memory.centiBytesPerCell`: that is what a *cell* costs,
    /// and this is what its formula costs on top. When the second is several
    /// times the first, a filled column is dominated by its trees and `PERF-11`
    /// is worth its risk.
    centi_arena_bytes_per_cell: u64,
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

/// What a million cells actually cost in memory, or why that is unknown.
///
/// `PERF-08` measured a real million-cell build, which is *payload* — the bytes
/// a document holds — and not what the process asks the operating system for.
/// The two differ by whatever the allocator, the index structures and the
/// interning add, and that difference is the whole question when the target is
/// "a million cells on a laptop".
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryReport {
    /// Whether this platform could be measured at all.
    ///
    /// Reported rather than omitted so a run on a machine with no `/proc` says
    /// so, instead of looking like a run where the number happened to be
    /// missing. macOS is that machine, which is why this had to be honest
    /// rather than convenient.
    available: bool,
    /// Peak resident set before the workbook was built, in bytes.
    baseline_bytes: u64,
    /// Peak resident set after it was built.
    peak_bytes: u64,
    /// The workbook's own cost: `peak - baseline`.
    workbook_bytes: u64,
    cells: u64,
    /// `workbook_bytes / cells`, in hundredths of a byte so the report stays
    /// integers.
    ///
    /// **This is the figure worth comparing across machines.** A byte is a byte
    /// on every runner, unlike a nanosecond — so unlike the timing gates, an
    /// absolute per-cell ceiling here is calibratable and does not need to be
    /// expressed as a ratio.
    centi_bytes_per_cell: u64,
}

/// Peak resident set size, from `/proc/self/status`.
///
/// `VmHWM` (high-water mark) rather than `VmRSS`: resident size falls when the
/// kernel reclaims pages, so a reading taken after a build can be lower than
/// what the build actually needed. The peak is what "will this fit" means.
///
/// **No counting allocator**, because `unsafe_code = "forbid"` is set
/// workspace-wide and `GlobalAlloc` cannot be implemented without `unsafe`.
/// Reading a file needs none, and this is the measurement the target is
/// expressed in anyway — the operating system's view, not the allocator's.
fn peak_resident_bytes() -> Option<u64> {
    parse_vm_hwm(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// Pull `VmHWM` out of a `/proc/self/status` body, in bytes.
///
/// Separated from the file read so it can be tested on a machine that has no
/// `/proc` — which is every developer machine here. The alternative was a
/// parser that only ever ran on CI, and a parser nobody can run is one nobody
/// can be sure of.
fn parse_vm_hwm(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

/// Turn two readings into a report.
///
/// Pure, and separated from the measuring for the same reason [`parse_vm_hwm`]
/// is: every machine here lacks `/proc`, so a decision made inside the
/// measurement could only ever be exercised on CI. The first version of this
/// *was* inside, and a mutation that broke the guard could not be caught
/// locally at all — the function returned at the no-`/proc` path long before
/// reaching it.
fn classify_memory(baseline: Option<u64>, peak: Option<u64>, cells: u32) -> MemoryReport {
    let unavailable = |baseline: u64, peak: u64| MemoryReport {
        available: false,
        baseline_bytes: baseline,
        peak_bytes: peak,
        workbook_bytes: 0,
        cells: u64::from(cells),
        centi_bytes_per_cell: 0,
    };
    let (Some(baseline), Some(peak)) = (baseline, peak) else {
        return unavailable(0, 0);
    };
    let grew = peak.saturating_sub(baseline);
    // **A build that cost nothing did not get measured.**
    //
    // `VmHWM` is a process-lifetime peak, so if anything larger ran first this
    // reads zero — indistinguishable from a workbook that is free, and reading
    // far more reassuringly. Reported as *unavailable* instead: "we could not
    // measure this" and "this costs nothing" must never look the same in a
    // report somebody sets a budget from.
    if grew == 0 || cells == 0 {
        return unavailable(baseline, peak);
    }
    MemoryReport {
        available: true,
        baseline_bytes: baseline,
        peak_bytes: peak,
        workbook_bytes: grew,
        cells: u64::from(cells),
        centi_bytes_per_cell: grew.saturating_mul(100) / u64::from(cells),
    }
}

/// Build a workbook of `cells` cells and report what it cost.
fn measure_memory(cells: u32) -> MemoryReport {
    let baseline = peak_resident_bytes();
    let workbook = build_workbook(cells);
    let peak = peak_resident_bytes();
    // Read the peak before dropping, and keep the workbook alive to here, or
    // the optimiser is entitled to have freed it already.
    let held = workbook.sheets.len();
    debug_assert!(held > 0 || cells == 0);
    classify_memory(baseline, peak, cells)
}

/// Count what a filled-down column's formula trees cost.
///
/// The column is `=A1*2`, `=A2*2`, … — one *shape*, filled down, which is the
/// case `docs/40` promised would share a tree and `PERF-09` cannot collapse
/// because the references shift and every row is therefore a different tree.
fn measure_arena(cells: u32) -> ArenaReport {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Filled");
    for row in 0..cells {
        sheet
            .cells
            .set(CellRef::new(row, 0), Cell::value(CellValue::Number(1.0)));
        let mut cell = Cell::value(CellValue::Number(0.0));
        // `store_formula_at`, which is the whole measurement: the same *shape*
        // filled down a column is one tree once references are stored relative
        // to the cell holding them (`PERF-11`).
        cell.formula = Some(workbook.store_formula_at(
            casual_calc_formula::parse(&format!("A{}*2", row + 1)).expect("parses"),
            casual_calc_formula::stored::Origin::at(row, 1),
        ));
        sheet.cells.set(CellRef::new(row, 1), cell);
    }
    workbook.sheets.push(sheet);

    let nodes: u64 = workbook.formulas.iter().map(|e| expr_nodes(e) as u64).sum();
    let expr_bytes = core::mem::size_of::<casual_calc_formula::Expr>() as u64;
    let arena_bytes = nodes * expr_bytes;
    ArenaReport {
        cells: u64::from(cells),
        distinct_trees: workbook.formulas.len() as u64,
        nodes,
        arena_bytes,
        expr_bytes,
        centi_arena_bytes_per_cell: if cells == 0 {
            0
        } else {
            arena_bytes * 100 / u64::from(cells)
        },
    }
}

/// Nodes in a tree, counted structurally.
fn expr_nodes(e: &casual_calc_formula::Expr) -> usize {
    use casual_calc_formula::Expr;
    1 + match e {
        Expr::Binary { left, right, .. } => expr_nodes(left) + expr_nodes(right),
        Expr::Unary { operand, .. } => expr_nodes(operand),
        Expr::Function { args, .. } => args.iter().map(expr_nodes).sum(),
        Expr::Call { callee, args } => {
            expr_nodes(callee) + args.iter().map(expr_nodes).sum::<usize>()
        }
        _ => 0,
    }
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

/// The worst-case incremental recalculation budget, from docs/30 T3.
///
/// **Absolute, like the frame budget and unlike everything else here.** The
/// scaling gates are ratios because absolute times on a shared runner say more
/// about the runner than the code — but "under fifty milliseconds" is not a
/// shape, it is a number, and a ratio cannot express it. T3 is a hard cap in
/// docs/30's own words, and until now nothing asserted it: the recalc cases
/// gated how the work *grew* and never how long it *took*, so the target CI is
/// documented as enforcing was not enforced at all.
const RECALC_BUDGET_NS: u64 = 50_000_000;

/// A worst-case recalculation, timed against [`RECALC_BUDGET_NS`].
///
/// The fixture is rebuilt per iteration, outside the clock, for the same reason
/// the scaling cases do it: an edit mutates the workbook, and the second edit of
/// the same cell measures a dirty set that is already clean.
fn measure_recalc<T>(
    id: &str,
    iterations: u32,
    setup: impl Fn() -> T,
    work: impl Fn(&mut T) -> u64,
) -> ScalingReport {
    let mut samples: Vec<u128> = (0..iterations)
        .map(|_| {
            let mut fixture = setup();
            let started = std::time::Instant::now();
            std::hint::black_box(work(&mut fixture));
            started.elapsed().as_nanos()
        })
        .collect();
    samples.sort_unstable();
    // The **worst** case, not the median. T3 says worst-case, and a median
    // hides exactly the tail a person notices — one edit in twenty taking half
    // a second is the complaint, and the median says everything is fine.
    let worst_ns = percentile(&samples, 1.0);
    ScalingReport {
        id: id.to_owned(),
        small_ns: worst_ns,
        large_ns: worst_ns,
        // Expressed against the budget so the report keeps one shape: 100 is
        // exactly at the cap.
        ratio_centi: worst_ns * 100 / RECALC_BUDGET_NS,
        budget_centi: 100,
        within_budget: worst_ns <= RECALC_BUDGET_NS,
    }
}

/// The adversarial workbook docs/30 T3 names: **a long dependency chain and a
/// wide fan-out**, on a sheet large enough that neither is lost in the noise.
///
/// Both shapes in one sheet because they fail differently. A chain is depth —
/// it defeats anything that recurses per level, and it is the shape a
/// running-total column has. A fan-out is breadth — one cell that thousands of
/// formulas read, which is what a rate or an exchange rate in a corner looks
/// like, and it is the edit that dirties the most dependents at once.
///
/// Column A is the chain: `A2 = A1+1`, `A3 = A2+1`, and so on. Column C is the
/// fan-out: every cell reads `A1`, the chain's root, so editing that one cell
/// dirties the whole chain *and* the whole fan at once — the worst edit this
/// sheet has.
fn build_adversarial_workbook(depth: u32, fan: u32) -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Adversarial");

    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
    for row in 1..depth {
        let mut cell = Cell::value(CellValue::Number(0.0));
        cell.formula = Some(
            workbook
                .store_formula(casual_calc_formula::parse(&format!("A{}+1", row)).expect("parses")),
        );
        sheet.cells.set(CellRef::new(row, 0), cell);
    }

    for row in 0..fan {
        let mut cell = Cell::value(CellValue::Number(0.0));
        cell.formula =
            Some(workbook.store_formula(casual_calc_formula::parse("A1*2").expect("parses")));
        sheet.cells.set(CellRef::new(row, 2), cell);
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
        // **The first edit after a document opens**, against the hard cap.
        //
        // The scaling case above measures the same work and asks only how it
        // grows. This asks the question docs/30 actually poses: does it finish
        // in fifty milliseconds. Cold on purpose — the precedent graph is built
        // inside the clock, because that is what the first edit pays for and it
        // is the one edit every session makes.
        measure_recalc(
            "recalc-cold-first-edit",
            iterations.max(5),
            || build_formula_workbook(10_000),
            |workbook| {
                let at = CellRef::new(0, 0);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(1.0)));
                casual_calc_eval::recalculate_incremental(workbook, &[(0, at)]);
                cheap_witness(workbook, at)
            },
        ),
        // **The worst edit the adversarial sheet has**, warm.
        //
        // Warm because T3 is about incremental recalculation, and a kept graph
        // is what "incremental" means; the cold case above already covers the
        // other half. The edit is on the chain's root, which is also what the
        // whole fan-out reads — so one keystroke dirties the depth and the
        // breadth at once, which is the worst case docs/30 defines rather than
        // one this benchmark found convenient.
        measure_recalc(
            "recalc-adversarial-chain-and-fanout",
            iterations.max(5),
            || {
                let mut workbook = build_adversarial_workbook(5_000, 5_000);
                let mut recalc = casual_calc_eval::Recalculator::new();
                let at = CellRef::new(0, 0);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(2.0)));
                recalc.recalculate(&mut workbook, &[(0, at)]);
                (workbook, recalc)
            },
            |(workbook, recalc)| {
                let at = CellRef::new(0, 0);
                workbook.sheets[0]
                    .cells
                    .set(at, Cell::value(CellValue::Number(3.0)));
                recalc.recalculate(workbook, &[(0, at)]);
                cheap_witness(workbook, at)
            },
        ),
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

    // **Memory first, before any other case runs.**
    //
    // `VmHWM` is a high-water mark for the *process*, not for this build. The
    // scaling cases build workbooks an order of magnitude larger, so measuring
    // afterwards compares a small build against a peak something else already
    // set — and reports that a million cells cost nothing. That is exactly what
    // the first CI run showed: `baselineBytes` and `peakBytes` identical to the
    // byte, and a workbook that apparently occupied no memory at all.
    //
    // The order therefore matters, which is a fragile thing to depend on. It is
    // depended on anyway because the alternative — re-exec'ing into a child
    // process — buys robustness at the cost of a failure mode in every sandbox
    // this runs in. What makes it safe is that the failure is *loud*: a real
    // build cannot cost zero bytes, so CI asserts the number is positive and
    // caught this on the first run rather than reporting a comforting zero.
    let memory = measure_memory(if smoke { 50_000 } else { 1_000_000 });
    let arena = measure_arena(if smoke { 5_000 } else { 100_000 });

    let cases = build_cases()
        .iter()
        .map(|bench| run_bench(bench, iterations))
        .collect();

    // A debug build measures a different program — forty times slower on the
    // frame — so its numbers cannot be read as a verdict on anything shipped.
    // Said on stderr, where it does not disturb the report on stdout.
    if cfg!(debug_assertions) {
        eprintln!(
            "warning: this is a debug build. The absolute budgets (the frame) \
             describe release code; rebuild with --release before believing them."
        );
    }
    let report = Report {
        schema_version: SCHEMA_VERSION,
        environment,
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        smoke,
        cases,
        scaling: measure_all_scaling(if smoke { 3 } else { 20 }),
        memory,
        arena,
    };

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    /// A real `/proc/self/status`, trimmed. The fields around `VmHWM` are kept
    /// because they are what a naive parser trips over: `VmHW` is a prefix of
    /// nothing, but `VmRSS` sits next to it and `VmPeak` looks similar.
    const STATUS: &str = "\
Name:\tbench
VmPeak:\t  812345 kB
VmSize:\t  712345 kB
VmHWM:\t   524288 kB
VmRSS:\t   412345 kB
Threads:\t1
";

    /// **The peak is read, in bytes, from the right line.**
    #[test]
    fn the_high_water_mark_is_parsed_in_bytes() {
        assert_eq!(parse_vm_hwm(STATUS), Some(524_288 * 1024));
    }

    /// **`VmHWM`, not `VmRSS` or `VmPeak`.**
    ///
    /// `VmRSS` falls when the kernel reclaims pages, so a reading taken after a
    /// build can be lower than what the build needed — the number would drift
    /// down on a busy runner and the gate would loosen itself. `VmPeak` is
    /// address space, which is not memory.
    #[test]
    fn neither_the_current_size_nor_the_address_space_is_used() {
        let got = parse_vm_hwm(STATUS).unwrap();
        assert_ne!(got, 412_345 * 1024, "VmRSS was read instead of VmHWM");
        assert_ne!(got, 812_345 * 1024, "VmPeak was read instead of VmHWM");
    }

    /// **A platform with no such field says so**, rather than reporting zero as
    /// though it had measured nothing being used.
    #[test]
    fn a_status_without_the_field_is_unavailable() {
        assert_eq!(parse_vm_hwm("Name:\tbench\nThreads:\t1\n"), None);
        assert_eq!(parse_vm_hwm(""), None);
    }

    /// **A peak that did not move is not a free workbook.**
    ///
    /// This is what the first CI run reported: `baselineBytes` and `peakBytes`
    /// identical to the byte, and a million cells apparently costing nothing —
    /// because `VmHWM` is a process-lifetime peak and the scaling cases had
    /// already set it. The reassuring reading is the dangerous one.
    #[test]
    fn a_peak_that_did_not_move_is_reported_as_unmeasured() {
        let report = classify_memory(Some(17_088_512), Some(17_088_512), 1_000_000);
        assert!(
            !report.available,
            "a build that moved no pages was reported as a successful measurement"
        );
        assert_eq!(report.workbook_bytes, 0);
        assert_eq!(report.centi_bytes_per_cell, 0);
    }

    /// **A peak that did move is measured, and divided per cell.**
    ///
    /// The control: a guard that rejected everything would satisfy the test
    /// above and measure nothing ever again.
    #[test]
    fn a_real_growth_is_measured_per_cell() {
        let report = classify_memory(Some(16_000_000), Some(66_000_000), 1_000_000);
        assert!(
            report.available,
            "a 50 MB growth was not treated as a measurement"
        );
        assert_eq!(report.workbook_bytes, 50_000_000);
        // 50 bytes a cell, in hundredths.
        assert_eq!(report.centi_bytes_per_cell, 5_000);
    }

    /// **An unreadable platform is unavailable, not zero-cost.**
    ///
    /// The distinction the row insisted on: a run on a machine with no `/proc`
    /// must not look like a run where a million cells cost nothing.
    #[test]
    fn an_unreadable_platform_is_not_reported_as_zero_cost() {
        for (base, peak) in [(None, None), (None, Some(1)), (Some(1), None)] {
            assert!(
                !classify_memory(base, peak, 1_000).available,
                "an unreadable reading was reported as a measurement"
            );
        }
    }
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

#[cfg(test)]
mod arena_tests {
    use super::*;

    /// **A filled-down column is one shape and N trees** — the premise `PERF-11`
    /// rests on, measured rather than assumed.
    ///
    /// `PERF-09` collapses *identical* trees, and a filled column produces none:
    /// the references shift, so `A1*2` and `A2*2` are different trees that
    /// dedup cannot touch. If this ever reports fewer trees than cells,
    /// something has started sharing them and the row's premise has moved.
    ///
    /// **Stage 3 landed, and this is the assertion inverted.** It was written
    /// to fail loudly at exactly this moment rather than quietly keep passing,
    /// and it did.
    #[test]
    fn a_filled_column_holds_one_tree() {
        let report = measure_arena(500);
        assert_eq!(
            report.distinct_trees, 1,
            "a filled column is one shape and must be one tree (PERF-11)"
        );
        assert_eq!(report.nodes, 3, "`A1*2` is three nodes, once");
        // The figure the row exists for: what a formula costs per cell, now
        // that five hundred of them share one tree.
        assert!(
            report.centi_arena_bytes_per_cell < 100,
            "a shared column costs more than a byte a cell: {report:?}"
        );
    }

    /// The arithmetic is checkable, so the headline figure cannot drift from
    /// what it claims to be.
    #[test]
    fn the_reported_bytes_follow_from_the_nodes() {
        let report = measure_arena(200);
        assert_eq!(report.arena_bytes, report.nodes * report.expr_bytes);
        assert_eq!(
            report.centi_arena_bytes_per_cell,
            report.arena_bytes * 100 / report.cells
        );
        // **The arena does not grow with the column.** Stronger than any
        // per-cell figure and independent of how many cells are measured:
        // twenty-five times the cells is the same one tree, which is the
        // property `PERF-11` exists to create.
        let small = measure_arena(200);
        let large = measure_arena(5_000);
        assert_eq!(
            small.arena_bytes, large.arena_bytes,
            "the arena grew with the column — the shape is no longer shared"
        );
        assert!(
            large.centi_arena_bytes_per_cell < small.centi_arena_bytes_per_cell,
            "a longer column must cost *less* per cell, not the same"
        );
    }

    /// An empty sheet does not divide by zero.
    #[test]
    fn no_cells_is_not_a_panic() {
        let report = measure_arena(0);
        assert_eq!(report.centi_arena_bytes_per_cell, 0);
    }
}
