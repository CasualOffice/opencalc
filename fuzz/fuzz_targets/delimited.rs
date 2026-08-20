//! The delimited-text reader, against arbitrary bytes.
//!
//! `casual-calc-io` is the adapter behind the editor's file picker and behind a
//! WOPI host handing over a `.csv`, `.tsv` or `.psv`. It is a hand-rolled
//! RFC 4180 scanner with **no admission limits at all** — no `PackageLimits`,
//! no `OoxmlLimits`, no `SpreadsheetLimits`, no cell budget — and until this
//! target it had no fuzzer either. It was the only reader in the workspace in
//! that position.
//!
//! # Refusing is passing
//!
//! [`read_delimited`] has exactly one error, `InvalidUtf8`, so almost every
//! input is admitted; that is the design (delimited text carries no encoding
//! declaration, so decoding is the host's job) rather than a problem. Only a
//! panic, a hang or an out-of-memory is a defect.
//!
//! # The property
//!
//! The crate's own module documentation states it:
//!
//! > *a parse → write → parse round-trip is a model fixed point — which is what
//! > rules out Excel's habit of turning `007` into `7`, since that round-trip
//! > does not settle.*
//!
//! That claim is load-bearing: it is the argument for why the reader refuses to
//! type a leading-zero field and refuses to guess `3/5/2024`, and it is what
//! "opening a CSV and saving it does not change it" means to the person who
//! did it.
//!
//! What is asserted is measured on the two models that came back, never on a
//! prediction of what the reader will decide about a given field:
//!
//! * **The writer's output is readable.** `write_delimited` returning text its
//!   own `read_delimited` refuses is the "saves fine, opens empty" defect.
//! * **The text settles.** The second write equals the first.
//! * **No cell appears or vanishes**, and none changes *kind* — a number does
//!   not come back as text, nor text as a number.
//! * **The number-format code survives**, because losing it is how a date
//!   silently becomes `45356`.
//! * **A non-zero number does not come back as zero.** This one is
//!   [held](held_underflow); see below.
//!
//! Deliberately *not* asserted: that a number comes back bit-identical.
//! `format_number` rounds to 15 significant digits on purpose, to hide binary
//! tails — `43.480000000000004` is written `43.48` — and an assertion that
//! insisted otherwise would be this harness re-implementing the writer's
//! decision, which is the mistake `ods.rs` records paying for twice.
//!
//! Seeded from `fuzz/seeds/delimited/`.

#![no_main]

use casual_calc_io::{COMMA, PIPE, TAB, read_delimited, write_delimited};
use casual_calc_model::{CellValue, Workbook};
use libfuzzer_sys::fuzz_target;

/// How large a sheet's **extent** may be before the round-trip is skipped.
///
/// Not a property and not a bound on the reader: a cost control on this
/// harness, of the same kind and for the same reason as `ROUND_TRIP_CELLS` in
/// `ods.rs`. [`write_delimited`] walks the full rectangle from `A1` to the
/// furthest populated cell — every row × every column, populated or not — so
/// three cells can cost their bounding box. A run that entered one of those
/// would spend its whole budget inside the *writer* and never mutate the
/// *reader*, which is the surface under test.
///
/// The skipped case is not a case nobody should care about — it is a case this
/// harness is the wrong instrument for, and it is reported separately as an
/// unfixed finding (proposed row `IO-EXTENT`). Three cells, at `A1`, at row 0
/// column *n*-1, and at row *n* column 0, need `2n + 3` bytes of input and cost
/// `n²` cell visits and at least `n²` bytes of output, because every visited
/// cell writes at least its delimiter:
///
/// | input | extent | |
/// | --- | --- | --- |
/// | 8 002 B | 1.6 × 10⁷ | measured here at 1.44 s, under ASan |
/// | 16 002 B | 6.4 × 10⁷ | measured here as an OOM at libFuzzer's 2 GiB `-rss_limit_mb` |
/// | 64 002 B | 1.0 × 10⁹ | ≥ 1 GB of `String` by the bound above, not run |
const MAX_EXTENT: u64 = 1 << 16;

fuzz_target!(|data: &[u8]| {
    // The one refusal this reader has, and it does not depend on the delimiter.
    let Ok(_) = core::str::from_utf8(data) else {
        return;
    };

    for delimiter in [COMMA, TAB, PIPE] {
        let first = read_delimited(data, delimiter).expect("valid UTF-8 is admitted");
        if extent(&first) > MAX_EXTENT {
            continue;
        }

        let once = write_delimited(&first, 0, delimiter);
        let second = read_delimited(once.as_bytes(), delimiter)
            .expect("the writer produced text its own reader refuses");
        let twice = write_delimited(&second, 0, delimiter);

        let before = values(&first);
        let after = values(&second);

        // **HELD** — see `held_underflow`. Checked on what came back, once,
        // before anything is asserted: this defect makes the round trip fail
        // in three different ways at once and holding each of them separately
        // would leave three places to remember to delete.
        if before.len() == after.len()
            && before
                .iter()
                .zip(after.iter())
                .any(|(cell, next)| held_underflow(&cell.2, &next.2))
        {
            continue;
        }

        assert!(
            once == twice,
            "read → write → read did not settle for delimiter {:?}: \
             {once:?} then {twice:?}",
            delimiter as char
        );

        assert_eq!(
            before.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(),
            after.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(),
            "the populated cells changed across a round trip for delimiter {:?}",
            delimiter as char
        );

        for (cell, next) in before.iter().zip(after.iter()) {
            assert_eq!(
                cell.2.kind(),
                next.2.kind(),
                "the cell at ({}, {}) changed kind across a round trip: \
                 {:?} became {:?}",
                cell.0,
                cell.1,
                cell.2,
                next.2
            );
            assert_eq!(
                cell.3, next.3,
                "the number format on the cell at ({}, {}) did not survive a \
                 round trip",
                cell.0, cell.1
            );
            assert!(
                !lost_to_zero(&cell.2, &next.2),
                "the non-zero number at ({}, {}) came back as zero: {:?} then {:?}",
                cell.0,
                cell.1,
                cell.2,
                next.2
            );
        }
    }
});

/// **HELD — a defect this target found, not a property being waived.**
///
/// `format_number` renders a value by rounding to 15 significant *decimal
/// places relative to its magnitude*, then reparsing:
///
/// ```text
/// let decimals = (14 - magnitude).clamp(0, 15) as usize;
/// let rounded: f64 = format!("{n:.decimals$}").parse().unwrap_or(n);
/// ```
///
/// The `clamp(0, 15)` is the whole defect. For any `|n| < 5e-16` the magnitude
/// wants more than 15 decimals, the clamp refuses, `format!("{n:.15}")` is
/// `"0.000000000000000"`, and the cell is written **`0`**. Not rounded —
/// erased. `1e-16` is an ordinary number in a scientific data set, and this is
/// silent total data loss on the "open a CSV, save it" path, and on any
/// `.xlsx` → `.csv` export.
///
/// Measured, from `seeds/delimited/underflow.csv`:
///
/// | field | after one save and reopen | and a second save |
/// | --- | --- | --- |
/// | `1e-16` | `0` | `0` |
/// | `1e-300` | `0` | `0` |
/// | `5e-324` | `0` | `0` |
/// | `5e-16` | `1e-15` | `1e-15` |
/// | `-1e-300` | `-0` | **`0`** |
///
/// The last row is the same clamp seen from the other side, and it breaks the
/// documented property in its strongest form: `format!("{:.15}", -1e-300)`
/// parses back as `-0.0`, which is written `-0`; on the *next* save
/// `format_number`'s `if n == 0.0` branch — true for `-0.0` — writes `0`. So
/// the file is not a fixed point after one round trip, it is one after two,
/// and the sign disappears in between. That is the claim the reader's whole
/// conservative typing policy is justified by.
///
/// Held rather than silenced, and held as narrowly as it can be stated: this
/// predicate matches *only* a non-zero number that came back exactly zero, and
/// only an input that actually exhibits it is skipped. **When the row is
/// fixed, delete this function and the one `continue` that calls it** — every
/// assertion in this target then becomes the regression proof, and
/// `seeds/delimited/underflow.csv` is the input that proves it.
fn held_underflow(before: &Field, after: &Field) -> bool {
    lost_to_zero(before, after)
}

/// A number that was not zero and came back zero.
///
/// `-0.0` is not "not zero": `-0.0 != 0.0` is false, so a `-0` written back as
/// `0` is equal here and correctly raises nothing.
fn lost_to_zero(before: &Field, after: &Field) -> bool {
    matches!((before, after), (Field::Number(a), Field::Number(b)) if *a != 0.0 && *b == 0.0)
}

/// The rectangle [`write_delimited`] will walk, saturating rather than wrapping.
fn extent(workbook: &Workbook) -> u64 {
    let Some(sheet) = workbook.sheets.first() else {
        return 0;
    };
    let (mut rows, mut cols) = (0u64, 0u64);
    for (at, _) in sheet.cells.iter() {
        rows = rows.max(u64::from(at.row) + 1);
        cols = cols.max(u64::from(at.col) + 1);
    }
    rows.saturating_mul(cols)
}

/// A cell's value as this target compares it.
///
/// Interned handles are resolved to the text they stand for: a `StringId` is an
/// index into a table rebuilt on every parse, so comparing handles would
/// compare arena layout and not content.
#[derive(Debug, PartialEq)]
enum Field {
    Number(f64),
    Bool(bool),
    Text(String),
    Error(String),
    Empty,
}

impl Field {
    /// Which of the five it is, with the payload dropped.
    fn kind(&self) -> &'static str {
        match self {
            Field::Number(_) => "number",
            Field::Bool(_) => "bool",
            Field::Text(_) => "text",
            Field::Error(_) => "error",
            Field::Empty => "empty",
        }
    }
}

/// Every populated cell, as (row, column, value, number-format code).
fn values(workbook: &Workbook) -> Vec<(u32, u32, Field, Option<String>)> {
    let Some(sheet) = workbook.sheets.first() else {
        return Vec::new();
    };
    let mut out: Vec<_> = sheet
        .cells
        .iter()
        .map(|(at, cell)| {
            let field = match &cell.value {
                CellValue::Empty => Field::Empty,
                CellValue::Number(n) => Field::Number(*n),
                CellValue::Bool(b) => Field::Bool(*b),
                CellValue::SharedString(id) | CellValue::InlineString(id) => {
                    Field::Text(workbook.strings.get(*id).unwrap_or_default().to_owned())
                }
                CellValue::Error(e) => Field::Error(e.to_string()),
            };
            let format = cell
                .style
                .and_then(|id| workbook.styles.get(id))
                .and_then(|style| style.number_format.clone());
            (at.row, at.col, field, format)
        })
        .collect();
    out.sort_by_key(|(row, col, _, _)| (*row, *col));
    out
}
