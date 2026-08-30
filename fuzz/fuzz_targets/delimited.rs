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
//! * **A non-zero number does not come back as zero.** This was `IO-01`, held
//!   here by a predicate and a `continue` until the row was fixed; both are
//!   gone and the assertion is now the regression proof.
//! * **A save is not much larger than what it read.** See
//!   [`MAX_AMPLIFICATION`] — this was `IO-02`.
//!
//! Deliberately *not* asserted: that a number comes back bit-identical.
//! `format_number` rounds to 15 significant digits on purpose, to hide binary
//! tails — `43.480000000000004` is written `43.48` — and an assertion that
//! insisted otherwise would be this harness re-implementing the writer's
//! decision, which is the mistake `ods.rs` records paying for twice.
//!
//! Seeded from `fuzz/seeds/delimited/`.

#![no_main]

use casual_calc_io::{read_delimited, write_delimited, COMMA, PIPE, TAB};
use casual_calc_model::{CellValue, Workbook};
use libfuzzer_sys::fuzz_target;

/// How many times its own input a save may be, before the fixed slack.
///
/// A property, not a cost control on this harness — the shape `ods.rs` reached
/// with `within_its_own_bound` once `ODS-05` was fixed, and the reason this one
/// no longer skips a large extent. Every cell in the model had to be *said* in
/// the input: a field at column `c` needed `c` delimiters on its line and a
/// cell at row `r` needed `r` line breaks above it. So a writer that emits the
/// sheet's populated cells writes on the order of what the reader was given,
/// and one that emits its bounding box does not.
///
/// The factor is slack for a field whose text legitimately grows — `1e20` is
/// four bytes in and twenty-one out — not a tolerance for amplification. What
/// it caught, `IO-02`, missed it by orders of magnitude rather than by a
/// margin: three cells at `A1`, `(0, n-1)` and `(n, 0)` need `2n + 3` bytes of
/// input, and the bounding-box writer spent `n²` cell lookups and at least `n²`
/// bytes on them, because every visited position writes at least its delimiter.
///
/// | input | extent | |
/// | --- | --- | --- |
/// | 8 002 B | 1.6 × 10⁷ | measured here at 1.44 s, under ASan |
/// | 16 002 B | 6.4 × 10⁷ | measured here as an OOM at libFuzzer's 2 GiB `-rss_limit_mb` |
/// | 64 002 B | 1.0 × 10⁹ | ≥ 1 GB of `String` by the bound above, not run |
const MAX_AMPLIFICATION: usize = 16;

/// Bytes a save may add on top of [`MAX_AMPLIFICATION`], for the CRLF a
/// one-field input still ends with.
const AMPLIFICATION_SLACK: usize = 64;

fuzz_target!(|data: &[u8]| {
    // The one refusal this reader has, and it does not depend on the delimiter.
    let Ok(_) = core::str::from_utf8(data) else {
        return;
    };

    for delimiter in [COMMA, TAB, PIPE] {
        let first = read_delimited(data, delimiter).expect("valid UTF-8 is admitted");

        let once = write_delimited(&first, 0, delimiter);
        assert!(
            once.len() <= MAX_AMPLIFICATION * data.len() + AMPLIFICATION_SLACK,
            "saving {} bytes of delimiter {:?} wrote {} bytes, from {} cells",
            data.len(),
            delimiter as char,
            once.len(),
            first.sheets[0].cells.len()
        );

        let second = read_delimited(once.as_bytes(), delimiter)
            .expect("the writer produced text its own reader refuses");
        let twice = write_delimited(&second, 0, delimiter);

        let before = values(&first);
        let after = values(&second);

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

/// A number that was not zero and came back zero.
///
/// `-0.0` is not "not zero": `-0.0 != 0.0` is false, so a `-0` written back as
/// `0` is equal here and correctly raises nothing.
fn lost_to_zero(before: &Field, after: &Field) -> bool {
    matches!((before, after), (Field::Number(a), Field::Number(b)) if *a != 0.0 && *b == 0.0)
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
