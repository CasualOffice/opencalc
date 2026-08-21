//! The oracle diff: OpenCalc's evaluator against LibreOffice Calc's.
//!
//! Every other test of the calc engine was written from the specification by
//! the same person who wrote the code being tested. That catches mistakes but
//! not **misreadings** — where the spec was understood wrongly, the test agrees
//! with the bug. An independent implementation does not share the misreading,
//! so a disagreement is evidence in a way a passing self-written test is not.
//!
//! # What a disagreement means, and what it does not
//!
//! The oracle is LibreOffice; the target is Excel. Where LibreOffice and Excel
//! themselves differ — and they do — matching LibreOffice is not proof of
//! anything. So this tool reports **divergence from an independent
//! implementation**, which is a strong signal and not a verdict. A difference
//! that turns out to be LibreOffice's is recorded in the corpus with
//! `@differs: <reason>` rather than silenced, and the run then fails if the
//! difference ever *disappears*, so a stale excuse cannot survive quietly.
//!
//! # How
//!
//! 1. Build a workbook: a fixture data block plus one corpus formula per row.
//! 2. Write it with our own writer, and hand it to `soffice --convert-to csv`,
//!    which loads, **recalculates**, and writes what it computed.
//! 3. Compare, numerically where both sides are numbers and textually
//!    otherwise.
//!
//! LibreOffice writes about fourteen significant digits, so the numeric
//! comparison is relative and loose enough to ignore that and far tighter than
//! any real disagreement: a wrong rounding rule or a wrong branch is not out by
//! one part in a trillion.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use casual_calc_formula::parse;
use casual_calc_formula::stored::Origin;
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use casual_calc_sdk::WorkbookSession;

/// The column the corpus formulas are written into.
const FORMULA_COL: u32 = 0;

/// The CSV export filter, with LibreOffice's own option tokens.
///
/// Comma separated, double quoted, UTF-8, from the first row, **as shown** —
/// the last of those matters: it is what LibreOffice displays, which is the
/// only view of a document a CSV can carry. Both oracle modes use this one
/// string so a change to how the oracle is read cannot apply to half of them.
const CSV_FILTER: &str =
    "csv:Text - txt - csv (StarCalc):44,34,76,1,,0,false,true,true,false,false,-1,false,true";

/// How far apart two numbers may be and still count as agreeing, relative to
/// the larger. LibreOffice exports about fourteen significant digits; this is
/// two orders looser than that and many orders tighter than a real defect.
const TOLERANCE: f64 = 1e-12;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = if args.iter().any(|a| a == "--validate-package") {
        validate_package(&args)
    } else if args.iter().any(|a| a == "--ods") {
        ods_corpus(&args)
    } else {
        run(&args)
    };
    match outcome {
        Ok(report) => {
            let failed = report.print();
            if let Some(path) = flag(&args, "--json") {
                let json = serde_json::to_string_pretty(&report).expect("serialize report");
                if let Err(e) = std::fs::write(&path, json) {
                    eprintln!("could not write {path}: {e}");
                    std::process::exit(2);
                }
            }
            std::process::exit(i32::from(failed));
        }
        Err(e) => {
            eprintln!("casual-calc-fidelity: {e}");
            std::process::exit(2);
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

/// One corpus entry.
struct Entry {
    formula: String,
    /// Why this one is expected to disagree, if it is.
    differs: Option<String>,
    line: usize,
}

/// What the two implementations said about one formula.
#[derive(serde::Serialize)]
struct Comparison {
    formula: String,
    ours: String,
    oracle: String,
    agreed: bool,
    /// The recorded reason this one is expected to disagree.
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_to_differ: Option<String>,
}

#[derive(serde::Serialize)]
struct Report {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    oracle: String,
    total: usize,
    agreed: usize,
    /// Disagreements that were not expected — these fail the run.
    unexpected: Vec<Comparison>,
    /// Disagreements the corpus records a reason for.
    known: Vec<Comparison>,
    /// Entries marked `@differs` that now agree, so the marker is stale.
    stale_markers: Vec<Comparison>,
    /// Both sides reported an error, but not the same token.
    ///
    /// Not a disagreement about the answer, and not an agreement either: it is
    /// the **limit of this oracle**. LibreOffice's CSV export writes its own
    /// internal codes — `Err:502` for an invalid argument, `Err:523` for a
    /// solver that will not converge — and SpreadsheetML has no token for
    /// either, so it cannot tell us whether `#NUM!` or `#VALUE!` was the right
    /// answer. What it does tell us is that both engines refused, which is the
    /// part worth knowing: a value where the oracle errors, or the reverse, is
    /// a real finding and still lands in `unexpected`.
    error_class_only: Vec<Comparison>,
}

impl Report {
    /// Print a human summary. Returns whether the run should fail.
    fn print(&self) -> bool {
        println!(
            "oracle: {} — {} formulas, {} agreed",
            self.oracle, self.total, self.agreed
        );

        for c in &self.error_class_only {
            println!(
                "  errors: {}\n           ours {}   oracle {}   (the oracle cannot name this one)",
                c.formula, c.ours, c.oracle
            );
        }
        for c in &self.known {
            println!(
                "  known: {}\n           ours {}   oracle {}   ({})",
                c.formula,
                c.ours,
                c.oracle,
                c.expected_to_differ.as_deref().unwrap_or("")
            );
        }
        for c in &self.stale_markers {
            println!(
                "  STALE: {} is marked @differs but the two now agree ({}); remove the marker",
                c.formula, c.ours
            );
        }
        for c in &self.unexpected {
            println!(
                "  DIFFER: {}\n           ours {}   oracle {}",
                c.formula, c.ours, c.oracle
            );
        }

        let failed = !self.unexpected.is_empty() || !self.stale_markers.is_empty();
        println!(
            "{}: {} unexpected, {} known, {} error-class, {} stale marker(s)",
            if failed { "FAIL" } else { "ok" },
            self.unexpected.len(),
            self.known.len(),
            self.error_class_only.len(),
            self.stale_markers.len()
        );
        failed
    }
}

/// The package validation: does an independent implementation *accept and
/// understand* the files we write?
///
/// P1B-003. The oracle mode above asks whether the evaluator is right; this
/// asks whether the writer is. They are different failures: a workbook can hold
/// every correct value and still be a package Excel offers to repair, or one
/// LibreOffice opens while quietly dropping half of what it was told.
///
/// The method is a full **re-save**, not merely an open: `soffice` is asked to
/// convert our `.xlsx` to `.xlsx`, which drives its reader over every part and
/// then writes what it understood. Re-importing that and comparing to what we
/// started with catches the structural mistakes an "it opened" check cannot —
/// a part LibreOffice skipped, a merge it did not see, a frozen pane it lost.
///
/// **Honest limit**: this is LibreOffice's acceptance, not Excel's. Excel's
/// repair prompt cannot be tested without Excel, and nothing here should be
/// read as saying otherwise.
fn validate_package(args: &[String]) -> Result<Report, String> {
    let soffice = flag(args, "--soffice").unwrap_or_else(|| "soffice".to_owned());
    let workbook = feature_workbook();
    let session = WorkbookSession::from_workbook(workbook);

    let dir = std::env::temp_dir().join(format!("casual-calc-package-{}", std::process::id()));
    let out = dir.join("resaved");
    std::fs::create_dir_all(&out).map_err(|e| format!("temp dir: {e}"))?;
    let ours = dir.join("ours.xlsx");
    std::fs::write(&ours, session.save().map_err(|e| format!("writing: {e}"))?)
        .map_err(|e| format!("writing {}: {e}", ours.display()))?;

    let version = Command::new(&soffice)
        .arg("--version")
        .output()
        .map_err(|e| format!("running {soffice}: {e} — is LibreOffice installed?"))
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())?;

    let profile = dir.join("profile");
    let result = Command::new(&soffice)
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .arg("--headless")
        .arg("--convert-to")
        .arg("xlsx")
        .arg("--outdir")
        .arg(&out)
        .arg(&ours)
        .output()
        .map_err(|e| format!("re-saving: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "soffice refused the package we wrote ({}): {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ));
    }

    let resaved = std::fs::read_dir(&out)
        .map_err(|e| format!("reading {}: {e}", out.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "xlsx"))
        .ok_or_else(|| "soffice wrote no xlsx — it did not accept the package".to_owned())?;

    let theirs = casual_calc_sdk::WorkbookSession::open(
        std::fs::read(&resaved).map_err(|e| format!("reading {resaved:?}: {e}"))?,
    )
    .map_err(|e| format!("re-importing LibreOffice's save: {e}"))?;

    let mut report = Report {
        schema_version: 1,
        oracle: version,
        total: 0,
        agreed: 0,
        unexpected: Vec::new(),
        known: Vec::new(),
        stale_markers: Vec::new(),
        error_class_only: Vec::new(),
    };

    for (property, mine, theirs) in survey(session.workbook(), theirs.workbook()) {
        report.total += 1;
        if mine == theirs {
            report.agreed += 1;
        } else if let Some(why) = known_normalisation(&mine, &theirs) {
            report.known.push(Comparison {
                formula: property,
                ours: mine,
                oracle: theirs,
                agreed: false,
                expected_to_differ: Some(why.to_owned()),
            });
        } else {
            report.unexpected.push(Comparison {
                formula: property,
                ours: mine,
                oracle: theirs,
                agreed: false,
                expected_to_differ: None,
            });
        }
    }
    Ok(report)
}

/// Differences a re-save through LibreOffice is *expected* to introduce.
///
/// Each is a rewrite LibreOffice performs on its own way out, not something it
/// failed to understand on the way in — the value survives, only the spelling
/// changes. They are listed rather than normalised away so they stay visible on
/// every run: if one of them stops happening, or a new one appears, that is
/// worth a look and not a silently widened comparison.
///
/// Deliberately narrow. Each rule matches one specific rewrite; nothing here
/// says "if the two are roughly similar, pass".
fn known_normalisation(ours: &str, theirs: &str) -> Option<&'static str> {
    // A boolean *literal* cell comes back as the formula `=TRUE()`.
    if (ours == "TRUE" || ours == "FALSE") && theirs == format!("={ours}()") {
        return Some("LibreOffice rewrites a boolean literal as the function TRUE()/FALSE()");
    }
    // An error *literal* cell comes back as a formula yielding that error.
    if ours.starts_with('#') && theirs == format!("={ours}") {
        return Some("LibreOffice rewrites an error literal as a formula");
    }
    // Inside a formula, the boolean literals become calls.
    if ours.starts_with('=')
        && theirs.starts_with('=')
        && theirs.replace("TRUE()", "TRUE").replace("FALSE()", "FALSE") == ours
    {
        return Some("LibreOffice writes the boolean literals in a formula as TRUE()/FALSE()");
    }
    None
}

/// A workbook exercising the structure a `.xlsx` has to carry.
///
/// Not a showcase: every item here is something `docs/18` claims is
/// implemented and gated, and the point is to make the writer produce all of it
/// in one package so a structural mistake has somewhere to show itself.
fn feature_workbook() -> Workbook {
    use casual_calc_model::{CellRange, DefinedName, Style};

    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let header = wb.intern_style(Style {
        bold: true,
        fill_color: Some("D9E7FF".to_owned()),
        ..Style::default()
    });
    let hello = wb.intern_string("hello");
    let banner = wb.intern_string("Merged banner");

    let mut first = Sheet::new(SheetId(Id::from_parts(2, 1)), "Data");
    // Every value kind, so none is lost in translation.
    first.cells.set(
        CellRef::new(0, 0),
        Cell {
            value: CellValue::SharedString(hello),
            style: Some(header),
            ..Cell::default()
        },
    );
    first
        .cells
        .set(CellRef::new(0, 1), Cell::value(CellValue::Number(42.5)));
    first
        .cells
        .set(CellRef::new(0, 2), Cell::value(CellValue::Bool(true)));
    first.cells.set(
        CellRef::new(0, 3),
        Cell::value(CellValue::Error(casual_calc_model::ErrorValue::Div0)),
    );
    for row in 1..6u32 {
        first.cells.set(
            CellRef::new(row, 0),
            Cell::value(CellValue::Number(f64::from(row))),
        );
        // `store_formula_at`: a formula is stored relative to the cell that
        // holds it (`PERF-11`), and a corpus built absolutely writes shifted
        // text into the package LibreOffice is then asked to agree with.
        let handle = wb.store_formula_at(
            parse(&format!("A{}*2", row + 1)).unwrap(),
            Origin::at(row, 1),
        );
        first.cells.set(
            CellRef::new(row, 1),
            Cell {
                formula: Some(handle),
                ..Cell::default()
            },
        );
    }
    // A future function, which is the class of thing FID-06 was about.
    let joined = wb.store_formula_at(
        parse("TEXTJOIN(\",\",TRUE,A2:A6)").unwrap(),
        Origin::at(7, 0),
    );
    first.cells.set(
        CellRef::new(7, 0),
        Cell {
            formula: Some(joined),
            ..Cell::default()
        },
    );
    // A merged banner, a freeze, a hidden row and a resized column.
    first.cells.set(
        CellRef::new(9, 0),
        Cell {
            value: CellValue::SharedString(banner),
            style: Some(header),
            ..Cell::default()
        },
    );
    first
        .merges
        .push(CellRange::new(CellRef::new(9, 0), CellRef::new(9, 3)));
    first.view.frozen_rows = 1;
    first.view.frozen_cols = 1;
    first.hidden_rows.insert(6);
    first.columns.sizes.insert(0, 3000);
    wb.sheets.push(first);

    // A second sheet, and a cross-sheet formula pointing back at the first.
    let mut second = Sheet::new(SheetId(Id::from_parts(2, 2)), "Summary");
    let total = wb.store_formula_at(parse("SUM(Data!A2:A6)").unwrap(), Origin::at(0, 0));
    second.cells.set(
        CellRef::new(0, 0),
        Cell {
            formula: Some(total),
            ..Cell::default()
        },
    );
    wb.sheets.push(second);

    wb.defined_names.push(DefinedName {
        name: "Values".to_owned(),
        sheet: None,
        formula: parse("Data!A2:A6").unwrap(),
    });
    wb
}

/// The properties compared across a re-save, as `(what, ours, theirs)`.
///
/// Deliberately about **structure and values**, not formatting: a re-save
/// through another application legitimately normalises styles, and asserting on
/// those would make the check fail for reasons nobody should act on.
fn survey(ours: &Workbook, theirs: &Workbook) -> Vec<(String, String, String)> {
    let mut out = vec![(
        "sheet count".to_owned(),
        ours.sheets.len().to_string(),
        theirs.sheets.len().to_string(),
    )];
    for (i, sheet) in ours.sheets.iter().enumerate() {
        let Some(other) = theirs.sheets.get(i) else {
            out.push((format!("sheet {i} exists"), "yes".into(), "no".into()));
            continue;
        };
        let at = |name: &str| format!("sheet {i} ({}) {name}", sheet.name);
        out.push((at("name"), sheet.name.clone(), other.name.clone()));
        out.push((
            at("merges"),
            format!("{:?}", sheet.merges),
            format!("{:?}", other.merges),
        ));
        out.push((
            at("frozen rows"),
            sheet.view.frozen_rows.to_string(),
            other.view.frozen_rows.to_string(),
        ));
        out.push((
            at("frozen cols"),
            sheet.view.frozen_cols.to_string(),
            other.view.frozen_cols.to_string(),
        ));
        out.push((
            at("hidden rows"),
            format!("{:?}", sheet.hidden_rows),
            format!("{:?}", other.hidden_rows),
        ));
        for (cell_ref, cell) in sheet.cells.row_band(0, u32::MAX) {
            let mine = describe_cell(ours, cell, cell_ref);
            let theirs_cell = other.cells.get(cell_ref).map_or_else(
                || "<missing>".to_owned(),
                |c| describe_cell(theirs, c, cell_ref),
            );
            out.push((
                format!(
                    "sheet {i} {}{}",
                    casual_calc_formula::column_to_letters(cell_ref.col),
                    cell_ref.row + 1
                ),
                mine,
                theirs_cell,
            ));
        }
    }
    out.push((
        "defined names".to_owned(),
        format!(
            "{:?}",
            ours.defined_names
                .iter()
                .map(|n| (&n.name, n.formula.to_string()))
                .collect::<Vec<_>>()
        ),
        format!(
            "{:?}",
            theirs
                .defined_names
                .iter()
                .map(|n| (&n.name, n.formula.to_string()))
                .collect::<Vec<_>>()
        ),
    ));
    out
}

/// A cell as its formula if it has one, else its value — the two things a
/// re-save must not change.
fn describe_cell(
    wb: &Workbook,
    cell: &casual_calc_model::Cell,
    at: casual_calc_model::CellRef,
) -> String {
    if let Some(handle) = cell.formula
        && let Some(expr) = wb.formula(handle)
    {
        // **Printed at the cell**, not absolutely (`PERF-11`). A stored tree's
        // references are offsets, so `Display` renders them measured from `A1`
        // — and a formula whose offsets reach above row 1 comes out as
        // `#REF!`. That is not merely ugly in the report: two *different*
        // formulas can both render `#REF!` and compare equal, so the check
        // quietly stops catching what it exists to catch.
        return format!(
            "={}",
            casual_calc_formula::print_at(expr, Origin::at(at.row, at.col))
        );
    }
    match &cell.value {
        CellValue::Empty => "<empty>".to_owned(),
        CellValue::Number(n) => format_number(*n),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            wb.strings.get(*id).unwrap_or_default().to_owned()
        }
        CellValue::Error(e) => e.to_string(),
    }
}

fn run(args: &[String]) -> Result<Report, String> {
    let corpus_path = flag(args, "--corpus").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("corpus")
                .join("formulas.txt")
        },
        PathBuf::from,
    );
    let soffice = flag(args, "--soffice").unwrap_or_else(|| "soffice".to_owned());
    let entries = read_corpus(&corpus_path)?;
    if entries.is_empty() {
        return Err(format!("no formulas in {}", corpus_path.display()));
    }

    let workbook = build_workbook(&entries)?;
    let session = WorkbookSession::from_workbook(workbook);

    let dir = std::env::temp_dir().join(format!("casual-calc-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
    let book = dir.join("corpus.xlsx");
    std::fs::write(
        &book,
        session.save().map_err(|e| format!("writing corpus: {e}"))?,
    )
    .map_err(|e| format!("writing {}: {e}", book.display()))?;

    let oracle = run_oracle(&soffice, &book, &dir)?;

    let mut report = Report {
        schema_version: 1,
        oracle: oracle.version,
        total: entries.len(),
        agreed: 0,
        unexpected: Vec::new(),
        known: Vec::new(),
        stale_markers: Vec::new(),
        error_class_only: Vec::new(),
    };

    for (row, entry) in entries.iter().enumerate() {
        let ours = render(
            &session,
            session.workbook().sheets[0]
                .cells
                .get(CellRef::new(row as u32, FORMULA_COL)),
        );
        let theirs = oracle
            .values
            .get(&(row as u32))
            .cloned()
            .unwrap_or_default();
        let agreed = agrees(&ours, &theirs);

        let comparison = Comparison {
            formula: entry.formula.clone(),
            ours,
            oracle: theirs,
            agreed,
            expected_to_differ: entry.differs.clone(),
        };
        // Error-class first: an entry where both sides refused is the oracle
        // declining to adjudicate, whatever the corpus says about it.
        if !agreed && is_error(&comparison.ours) && is_error(&comparison.oracle) {
            report.error_class_only.push(comparison);
            continue;
        }
        match (agreed, entry.differs.is_some()) {
            (true, false) => report.agreed += 1,
            (true, true) => report.stale_markers.push(comparison),
            (false, true) => report.known.push(comparison),
            (false, false) => report.unexpected.push(comparison),
        }
    }
    let _ = entries.iter().map(|e| e.line);
    Ok(report)
}

fn read_corpus(path: &Path) -> Result<Vec<Entry>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (formula, differs) = match line.split_once("@differs:") {
            Some((f, why)) => (f.trim(), Some(why.trim().to_owned())),
            None => (line, None),
        };
        out.push(Entry {
            formula: formula.to_owned(),
            differs,
            line: i + 1,
        });
    }
    Ok(out)
}

/// The workbook the two implementations are both asked about.
///
/// The fixture block is written first and is documented at the head of the
/// corpus file, so a formula can reference real data rather than being confined
/// to literals — which is where the interesting disagreements live.
fn build_workbook(entries: &[Entry]) -> Result<Workbook, String> {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");

    for row in 0..10u32 {
        let n = f64::from(row + 1);
        sheet
            .cells
            .set(CellRef::new(row, 2), Cell::value(CellValue::Number(n)));
        sheet.cells.set(
            CellRef::new(row, 3),
            Cell::value(CellValue::Number(n * 10.0)),
        );
    }
    for (row, word) in ["alpha", "beta", "gamma", "delta", "epsilon"]
        .iter()
        .enumerate()
    {
        let id = wb.intern_string(word);
        sheet.cells.set(
            CellRef::new(row as u32, 4),
            Cell::value(CellValue::SharedString(id)),
        );
    }
    for (row, flag) in [true, false, true, false, true].iter().enumerate() {
        sheet.cells.set(
            CellRef::new(row as u32, 5),
            Cell::value(CellValue::Bool(*flag)),
        );
    }
    for (row, n) in [2.5f64, -3.5, 0.0, 1e10, -0.25].iter().enumerate() {
        sheet.cells.set(
            CellRef::new(row as u32, 6),
            Cell::value(CellValue::Number(*n)),
        );
    }
    // Text that looks numeric, which is where coercion rules show themselves.
    for (row, text) in ["10", "-2.5", "abc", "", "3"].iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let id = wb.intern_string(text);
        sheet.cells.set(
            CellRef::new(row as u32, 7),
            Cell::value(CellValue::SharedString(id)),
        );
    }

    for (row, entry) in entries.iter().enumerate() {
        let expr = parse(&entry.formula)
            .map_err(|e| format!("line {}: cannot parse {:?}: {e}", entry.line, entry.formula))?;
        // **Stored at the cell it is written into** (`PERF-11`). Each corpus
        // line lands on its own row, so a formula interned absolutely resolves
        // `C1:C10` shifted down by that row — which is why the lookups came
        // back empty against an oracle that had values.
        let handle = wb.store_formula_at(expr, Origin::at(row as u32, FORMULA_COL));
        sheet.cells.set(
            CellRef::new(row as u32, FORMULA_COL),
            Cell {
                formula: Some(handle),
                ..Cell::default()
            },
        );
    }

    wb.sheets.push(sheet);
    Ok(wb)
}

struct Oracle {
    version: String,
    /// Row index → the text LibreOffice computed for column A.
    values: BTreeMap<u32, String>,
}

fn run_oracle(soffice: &str, book: &Path, dir: &Path) -> Result<Oracle, String> {
    let version = Command::new(soffice)
        .arg("--version")
        .output()
        .map_err(|e| format!("running {soffice}: {e} — is LibreOffice installed?"))
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())?;

    // A dedicated user profile, so a converter run never contends with an
    // interactive LibreOffice the developer happens to have open — which
    // otherwise fails with an unhelpful "javaldx" or simply hangs.
    let profile = dir.join("profile");
    let status = Command::new(soffice)
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .arg("--headless")
        .arg("--convert-to")
        .arg(CSV_FILTER)
        .arg("--outdir")
        .arg(dir)
        .arg(book)
        .status()
        .map_err(|e| format!("converting: {e}"))?;
    if !status.success() {
        return Err(format!("soffice exited with {status}"));
    }

    // LibreOffice names the output after the sheet when a book has one sheet
    // with a name, and after the file otherwise; take whichever csv appeared.
    let csv = std::fs::read_dir(dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "csv"))
        .ok_or_else(|| format!("soffice wrote no csv into {}", dir.display()))?;
    let text = std::fs::read_to_string(&csv).map_err(|e| format!("reading {csv:?}: {e}"))?;

    let mut values = BTreeMap::new();
    for (row, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        values.insert(row as u32, first_field(line));
    }
    Ok(Oracle { version, values })
}

/// The first comma-separated field of a CSV line, unquoting if it is quoted.
fn first_field(line: &str) -> String {
    if let Some(rest) = line.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    out.push('"');
                } else {
                    break;
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    line.split(',').next().unwrap_or_default().to_owned()
}

/// Our computed value, rendered the way the oracle renders its own.
///
/// Deliberately not the number-format pipeline: this compares **values**, and
/// putting a formatter on one side of the comparison would make a formatting
/// difference look like a calculation difference.
fn render(session: &WorkbookSession, cell: Option<&Cell>) -> String {
    let Some(cell) = cell else {
        return String::new();
    };
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => format_number(*n),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => session
            .workbook()
            .strings
            .get(*id)
            .unwrap_or_default()
            .to_owned(),
        CellValue::Error(e) => e.to_string(),
    }
}

/// A number as text, without exponent gymnastics — the comparison re-parses
/// both sides anyway, so this only has to round-trip.
fn format_number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Whether the two sides agree: numerically if both are numbers, textually if
/// not.
fn agrees(ours: &str, theirs: &str) -> bool {
    if ours == theirs {
        return true;
    }
    match (parse_number(ours), parse_number(theirs)) {
        (Some(a), Some(b)) => close(a, b),
        _ => false,
    }
}

/// Whether a rendered value is an error, in either implementation's spelling.
///
/// Ours are SpreadsheetML tokens (`#VALUE!`); LibreOffice's CSV export writes
/// its own numbered codes (`Err:502`), which the format has no token for.
fn is_error(s: &str) -> bool {
    s.starts_with('#') || s.starts_with("Err:")
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() {
        return false;
    }
    let scale = a.abs().max(b.abs());
    if scale == 0.0 {
        return true;
    }
    (a - b).abs() / scale <= TOLERANCE
}

/// The `.ods` corpus, against the application that wrote it.
///
/// `ODS-02`. The ODS reader was covered by tests over documents this repository
/// produced, which proves the reader agrees with the writer and nothing about
/// whether either agrees with LibreOffice — the application that actually makes
/// `.ods` files, and now an upload path into this engine. Four of the five
/// defects found in that reader by hand were parser-shaped, and every one of
/// them is a mistake a real LibreOffice document walks into: it compresses runs
/// of empty cells into repeat attributes, escapes text, writes leading spaces
/// as elements rather than characters, and closes empty cells in place.
///
/// Two halves, both diffed against the same oracle:
///
/// * **Read** — LibreOffice converts the corpus document to CSV, which is what
///   it thinks the document says; our reader imports the same bytes; the values
///   are compared cell by cell. A cell in the wrong column shows up here as two
///   divergences and nowhere else.
/// * **Re-save** — our writer writes the workbook back out and LibreOffice
///   converts *that* to CSV. Comparing it against the first grid asks whether
///   the document still says the same thing after a round trip through this
///   engine, which is the question a user asks by opening their file again.
///
/// Divergences the corpus records a reason for are reported and do not fail the
/// run; a recorded divergence that stops happening **does** fail it, so a
/// reason cannot outlive the defect it excuses.
///
/// **Honest limits.** `--convert-to csv` exports the first sheet only, so the
/// corpus documents have one sheet each and nothing here tests multi-sheet
/// placement. And CSV carries what LibreOffice *shows*, so a cell whose number
/// format changes what is displayed compares as displayed — the corpus is
/// deliberately built from plainly formatted values for that reason.
fn ods_corpus(args: &[String]) -> Result<Report, String> {
    let soffice = flag(args, "--soffice").unwrap_or_else(|| "soffice".to_owned());
    let dir = flag(args, "--ods-corpus").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("corpus")
                .join("ods")
        },
        PathBuf::from,
    );
    let mut expected = read_divergences(&dir.join("known-divergences.txt"))?;
    let mut documents: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ods"))
        .collect();
    documents.sort();
    if documents.is_empty() {
        return Err(format!(
            "no .ods documents in {} — run corpus/ods/generate.sh",
            dir.display()
        ));
    }

    let version = soffice_version(&soffice)?;
    let work = std::env::temp_dir().join(format!("casual-calc-ods-{}", std::process::id()));
    let mut report = Report {
        schema_version: 1,
        oracle: version,
        total: 0,
        agreed: 0,
        unexpected: Vec::new(),
        known: Vec::new(),
        stale_markers: Vec::new(),
        error_class_only: Vec::new(),
    };

    for (i, document) in documents.iter().enumerate() {
        let name = document
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let bytes =
            std::fs::read(document).map_err(|e| format!("reading {}: {e}", document.display()))?;
        let (workbook, _loss) = casual_calc_ods::import_ods(&bytes)
            .map_err(|e| format!("{name}: our reader refused a LibreOffice document: {e}"))?;

        let theirs = oracle_grid(&soffice, document, &work.join(format!("read-{i}")))?;
        compare_grids(
            &mut report,
            &mut expected,
            &name,
            &our_grid(&workbook),
            &theirs,
        );

        // The writer half. It goes through the same oracle so that a placement
        // mistake on the way out is measured the same way as one on the way in.
        let written = casual_calc_ods::export_ods(&workbook)
            .map_err(|e| format!("{name}: writing it back: {e}"))?;
        let out = work.join(format!("written-{i}"));
        std::fs::create_dir_all(&out).map_err(|e| format!("temp dir: {e}"))?;
        let path = out.join(&name);
        std::fs::write(&path, written).map_err(|e| format!("writing {}: {e}", path.display()))?;
        let round = oracle_grid(&soffice, &path, &out.join("csv"))?;
        compare_grids(
            &mut report,
            &mut expected,
            &format!("{name} re-saved"),
            &round,
            &theirs,
        );
    }

    // A reason recorded for a cell that no longer exists is as stale as one for
    // a divergence that stopped happening.
    for (key, reason) in expected {
        report.stale_markers.push(Comparison {
            formula: key,
            ours: "<no such cell>".to_owned(),
            oracle: String::new(),
            agreed: false,
            expected_to_differ: Some(reason),
        });
    }
    Ok(report)
}

/// The corpus's record of what is expected to differ, and why.
///
/// One `document.ods A2 @differs: reason` per line, `#` for comments — the same
/// shape as the formula corpus, for the same reason: a divergence with a
/// written reason is a decision, and a silenced one is a decision nobody made.
fn read_divergences(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    let mut out = BTreeMap::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, reason) = line.split_once("@differs:").ok_or_else(|| {
            format!(
                "{}:{}: every line needs `@differs: <reason>`",
                path.display(),
                i + 1
            )
        })?;
        out.insert(key.trim().to_owned(), reason.trim().to_owned());
    }
    Ok(out)
}

/// Our imported workbook's first sheet, as `(row, col) -> displayed value`.
fn our_grid(workbook: &Workbook) -> BTreeMap<(u32, u32), String> {
    let mut out = BTreeMap::new();
    let Some(sheet) = workbook.sheets.first() else {
        return out;
    };
    for (at, cell) in sheet.cells.iter() {
        let text = render_value(workbook, cell);
        if !text.is_empty() {
            out.insert((at.row, at.col), text);
        }
    }
    out
}

/// A cell as its value, the way the oracle prints its own.
///
/// Values, not formulas: what a `.ods` carries for a formula cell is the answer
/// LibreOffice last computed, and comparing our *formula text* against that
/// would be comparing two different things. Whether we compute the same answer
/// is the oracle-diff mode's question, not this one's.
fn render_value(workbook: &Workbook, cell: &Cell) -> String {
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => format_number(*n),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            workbook.strings.get(*id).unwrap_or_default().to_owned()
        }
        CellValue::Error(e) => e.to_string(),
    }
}

/// Compare two grids cell by cell, classifying every difference.
fn compare_grids(
    report: &mut Report,
    expected: &mut BTreeMap<String, String>,
    prefix: &str,
    ours: &BTreeMap<(u32, u32), String>,
    theirs: &BTreeMap<(u32, u32), String>,
) {
    let mut cells: Vec<(u32, u32)> = ours.keys().chain(theirs.keys()).copied().collect();
    cells.sort_unstable();
    cells.dedup();

    for at in cells {
        let mine = ours.get(&at).cloned().unwrap_or_default();
        let other = theirs.get(&at).cloned().unwrap_or_default();
        let property = format!(
            "{prefix} {}{}",
            casual_calc_formula::column_to_letters(at.1),
            at.0 + 1
        );
        let agreed = agrees(&mine, &other);
        let reason = expected.remove(&property);
        report.total += 1;
        let comparison = Comparison {
            formula: property,
            ours: mine,
            oracle: other,
            agreed,
            expected_to_differ: reason.clone(),
        };
        match (agreed, reason.is_some()) {
            (true, false) => report.agreed += 1,
            (true, true) => report.stale_markers.push(comparison),
            (false, true) => report.known.push(comparison),
            (false, false) => report.unexpected.push(comparison),
        }
    }
}

/// What LibreOffice thinks a spreadsheet says, as `(row, col) -> text`.
fn oracle_grid(
    soffice: &str,
    document: &Path,
    dir: &Path,
) -> Result<BTreeMap<(u32, u32), String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("temp dir: {e}"))?;
    let profile = dir.join("profile");
    let status = Command::new(soffice)
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .arg("--headless")
        .arg("--convert-to")
        .arg(CSV_FILTER)
        .arg("--outdir")
        .arg(dir)
        .arg(document)
        .status()
        .map_err(|e| format!("converting {}: {e}", document.display()))?;
    if !status.success() {
        return Err(format!(
            "soffice exited with {status} converting {}",
            document.display()
        ));
    }
    let csv = std::fs::read_dir(dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "csv"))
        .ok_or_else(|| {
            format!(
                "soffice wrote no csv for {} — it did not accept the document",
                document.display()
            )
        })?;
    let text = std::fs::read_to_string(&csv).map_err(|e| format!("reading {csv:?}: {e}"))?;

    let mut out = BTreeMap::new();
    for (row, fields) in csv_grid(text.trim_start_matches('\u{feff}'))
        .into_iter()
        .enumerate()
    {
        for (col, field) in fields.into_iter().enumerate() {
            if !field.is_empty() {
                out.insert((row as u32, col as u32), field);
            }
        }
    }
    Ok(out)
}

fn soffice_version(soffice: &str) -> Result<String, String> {
    Command::new(soffice)
        .arg("--version")
        .output()
        .map_err(|e| format!("running {soffice}: {e} — is LibreOffice installed?"))
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// A whole CSV, quoting and all.
///
/// The first-field reader above is not enough here: this compares every column,
/// and a corpus that exercises text edges necessarily contains the separator,
/// the quote character and a **line break inside a cell** — which is exactly
/// the input a `split(',')` would silently mangle into extra rows.
fn csv_grid(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => quoted = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_agree_within_the_oracles_printed_precision() {
        // LibreOffice writes about fourteen significant digits, so our full
        // double and its printed form must still count as agreeing.
        assert!(agrees("1.4142135623730951", "1.4142135623731"));
        // And a real difference must not.
        assert!(!agrees("2", "3"));
        assert!(!agrees("2.57", "2.58"));
    }

    #[test]
    fn text_and_errors_compare_exactly() {
        assert!(agrees("#DIV/0!", "#DIV/0!"));
        assert!(!agrees("#DIV/0!", "#VALUE!"));
        assert!(agrees("yes", "yes"));
        assert!(!agrees("yes", "no"));
    }

    #[test]
    fn the_normalisation_allowances_are_narrow() {
        // Each matches one specific rewrite. The risk with an allowance list is
        // that it grows into "close enough", so these pin what it must *not*
        // forgive as firmly as what it must.
        assert!(known_normalisation("TRUE", "=TRUE()").is_some());
        assert!(known_normalisation("#DIV/0!", "=#DIV/0!").is_some());
        assert!(known_normalisation("=IF(A1,TRUE,FALSE)", "=IF(A1,TRUE(),FALSE())").is_some());

        assert!(
            known_normalisation("TRUE", "FALSE").is_none(),
            "a flipped boolean is not a spelling difference"
        );
        assert!(
            known_normalisation("#DIV/0!", "=#VALUE!").is_none(),
            "nor is a different error"
        );
        assert!(
            known_normalisation("42", "=42").is_none(),
            "nor is a value silently becoming a formula"
        );
        assert!(
            known_normalisation("=SUM(A1:A3)", "=SUM(A1:A2)").is_none(),
            "nor is a changed range"
        );
    }

    #[test]
    fn both_spellings_of_an_error_are_recognised_as_one() {
        assert!(is_error("#VALUE!"));
        assert!(is_error("#N/A"));
        assert!(is_error("Err:502"));
        assert!(!is_error("502"));
        assert!(
            !is_error("error"),
            "a cell may legitimately contain the word"
        );
        assert!(!is_error(""));
    }

    #[test]
    fn a_number_and_a_word_never_agree() {
        assert!(!agrees("1", "one"));
        assert!(!agrees("", "0"));
    }

    #[test]
    fn a_quoted_csv_field_is_unquoted() {
        assert_eq!(first_field(r#""a,b",c"#), "a,b");
        assert_eq!(first_field(r#""say ""hi""",2"#), r#"say "hi""#);
        assert_eq!(first_field("plain,2"), "plain");
        assert_eq!(first_field(""), "");
    }

    /// A whole CSV, not a first field — and the case that matters is the one a
    /// `split(',')` gets wrong.
    #[test]
    fn a_csv_grid_survives_quotes_commas_and_a_line_break_inside_a_cell() {
        let grid = csv_grid("a,b\n\"one, two\",3\n\"first\nsecond\",4\n");
        assert_eq!(grid.len(), 3, "a quoted line break is one row, not two");
        assert_eq!(grid[0], vec!["a", "b"]);
        assert_eq!(
            grid[1],
            vec!["one, two", "3"],
            "a quoted separator is data, not a column boundary"
        );
        assert_eq!(
            grid[2],
            vec!["first\nsecond", "4"],
            "LibreOffice writes a multi-paragraph cell as one quoted field"
        );
    }

    #[test]
    fn a_doubled_quote_inside_a_csv_field_is_one_quote() {
        let grid = csv_grid("\"say \"\"hi\"\"\",2\n");
        assert_eq!(grid[0], vec![r#"say "hi""#, "2"]);
    }

    /// Trailing empty columns are what a sparse sheet exports as, and they must
    /// not be mistaken for the end of the row.
    #[test]
    fn empty_csv_fields_keep_their_column_positions() {
        let grid = csv_grid("left,,,right\n");
        assert_eq!(grid[0], vec!["left", "", "", "right"]);
    }

    #[test]
    fn a_recorded_divergence_is_read_with_its_reason() {
        let dir = std::env::temp_dir().join("casual-calc-fidelity-divergences-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known.txt");
        std::fs::write(
            &path,
            "# a comment\n\nbook.ods A2 @differs: spaces are elements\n             book.ods re-saved D2 @differs: no xmlns:of\n",
        )
        .unwrap();

        let known = read_divergences(&path).unwrap();
        assert_eq!(known.len(), 2, "comments and blanks are skipped");
        assert_eq!(
            known.get("book.ods A2").map(String::as_str),
            Some("spaces are elements")
        );
        assert_eq!(
            known.get("book.ods re-saved D2").map(String::as_str),
            Some("no xmlns:of")
        );
        // A missing file is not an error: a corpus with nothing to excuse is
        // the state this is trying to get back to.
        assert!(
            read_divergences(&dir.join("absent.txt"))
                .unwrap()
                .is_empty()
        );
    }

    /// A line without a reason is refused rather than read as one — a bare cell
    /// reference would otherwise silence a divergence with no explanation at
    /// all, which is the one thing this file exists to prevent.
    #[test]
    fn a_divergence_without_a_reason_is_refused() {
        let dir = std::env::temp_dir().join("casual-calc-fidelity-divergences-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("reasonless.txt");
        std::fs::write(&path, "book.ods A2\n").unwrap();
        assert!(read_divergences(&path).is_err());
    }

    /// The four outcomes, over one grid each way: agreed, an unexplained
    /// difference, an explained one, and a reason that has outlived its defect.
    #[test]
    fn every_divergence_is_classified_and_a_stale_reason_fails() {
        let mut report = Report {
            schema_version: 1,
            oracle: "test".to_owned(),
            total: 0,
            agreed: 0,
            unexpected: Vec::new(),
            known: Vec::new(),
            stale_markers: Vec::new(),
            error_class_only: Vec::new(),
        };
        let ours: BTreeMap<(u32, u32), String> = [
            ((0, 0), "same".to_owned()),
            ((1, 0), "ours".to_owned()),
            ((2, 0), "explained".to_owned()),
            ((3, 0), "agrees now".to_owned()),
        ]
        .into_iter()
        .collect();
        let theirs: BTreeMap<(u32, u32), String> = [
            ((0, 0), "same".to_owned()),
            ((1, 0), "theirs".to_owned()),
            ((2, 0), "different".to_owned()),
            ((3, 0), "agrees now".to_owned()),
        ]
        .into_iter()
        .collect();
        let mut expected: BTreeMap<String, String> = [
            ("b.ods A3".to_owned(), "a reason".to_owned()),
            (
                "b.ods A4".to_owned(),
                "an excuse that outlived it".to_owned(),
            ),
        ]
        .into_iter()
        .collect();

        compare_grids(&mut report, &mut expected, "b.ods", &ours, &theirs);

        assert_eq!(report.agreed, 1);
        assert_eq!(
            report.unexpected.len(),
            1,
            "an unexplained difference fails"
        );
        assert_eq!(report.unexpected[0].formula, "b.ods A2");
        assert_eq!(report.known.len(), 1, "an explained one does not");
        assert_eq!(report.stale_markers.len(), 1, "and a stale reason fails");
        assert_eq!(report.stale_markers[0].formula, "b.ods A4");
        assert!(report.print(), "a stale marker must fail the run");
        assert!(
            expected.is_empty(),
            "every reason used is consumed, so what is left never matched"
        );
    }

    /// A cell one side has and the other does not is a divergence, not a hole:
    /// the defect this corpus is aimed at moves a cell to the wrong column,
    /// which reads as missing here and extra there.
    #[test]
    fn a_cell_only_one_side_has_is_a_divergence() {
        let mut report = Report {
            schema_version: 1,
            oracle: "test".to_owned(),
            total: 0,
            agreed: 0,
            unexpected: Vec::new(),
            known: Vec::new(),
            stale_markers: Vec::new(),
            error_class_only: Vec::new(),
        };
        let ours: BTreeMap<(u32, u32), String> =
            [((0, 5), "far right".to_owned())].into_iter().collect();
        let theirs: BTreeMap<(u32, u32), String> =
            [((0, 9), "far right".to_owned())].into_iter().collect();
        compare_grids(&mut report, &mut BTreeMap::new(), "b.ods", &ours, &theirs);
        assert_eq!(
            report.unexpected.len(),
            2,
            "a cell in the wrong column diverges twice: where it is and where it should be"
        );
    }

    #[test]
    fn a_differs_marker_is_read_off_the_line() {
        let dir = std::env::temp_dir().join("casual-calc-fidelity-corpus-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.txt");
        std::fs::write(
            &path,
            "# a comment\n\n1+1\nIRR(A1:A5)  @differs: iterative seed\n",
        )
        .unwrap();

        let entries = read_corpus(&path).unwrap();
        assert_eq!(entries.len(), 2, "comments and blanks are skipped");
        assert_eq!(entries[0].formula, "1+1");
        assert!(entries[0].differs.is_none());
        assert_eq!(entries[1].formula, "IRR(A1:A5)");
        assert_eq!(entries[1].differs.as_deref(), Some("iterative seed"));
    }
}
