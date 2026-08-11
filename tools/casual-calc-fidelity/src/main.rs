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
use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};
use casual_calc_sdk::WorkbookSession;

/// The column the corpus formulas are written into.
const FORMULA_COL: u32 = 0;

/// How far apart two numbers may be and still count as agreeing, relative to
/// the larger. LibreOffice exports about fourteen significant digits; this is
/// two orders looser than that and many orders tighter than a real defect.
const TOLERANCE: f64 = 1e-12;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
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
        let handle = wb.store_formula(expr);
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
        .arg("csv:Text - txt - csv (StarCalc):44,34,76,1,,0,false,true,true,false,false,-1,false,true")
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
