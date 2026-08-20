//! Delimited-format tests: field typing, RFC 4180 quoting, and the
//! parse → write → parse round-trip fixed point.

use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook};

use crate::{COMMA, PIPE, TAB, read_delimited, write_delimited};

/// A one-sheet workbook holding exactly the cells given, by address.
fn workbook_of(cells: &[(u32, u32, CellValue)]) -> Workbook {
    let mut wb = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    for (row, col, value) in cells {
        sheet
            .cells
            .set(CellRef::new(*row, *col), Cell::value(value.clone()));
    }
    wb.sheets.push(sheet);
    wb
}

/// The field a single number is written as.
fn written_number(n: f64) -> String {
    let wb = workbook_of(&[(0, 0, CellValue::Number(n))]);
    let written = write_delimited(&wb, 0, COMMA);
    written
        .strip_suffix("\r\n")
        .expect("a row ends with CRLF")
        .to_owned()
}

/// The number in a cell, as bits — `-0.0 == 0.0` is true, so comparing the
/// values would let a lost sign through, which is half of `IO-01`.
fn number_bits(wb: &Workbook, r: u32, c: u32) -> u64 {
    match value(wb, r, c) {
        CellValue::Number(n) => n.to_bits(),
        other => panic!("expected a number at ({r}, {c}), got {other:?}"),
    }
}

fn value(wb: &Workbook, r: u32, c: u32) -> CellValue {
    wb.sheets[0]
        .cells
        .get(CellRef::new(r, c))
        .map(|cell| cell.value.clone())
        .unwrap_or(CellValue::Empty)
}

/// The resolved text of a string cell (panics if it isn't a string).
fn text(wb: &Workbook, r: u32, c: u32) -> String {
    match value(wb, r, c) {
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            wb.strings.get(id).unwrap().to_owned()
        }
        other => panic!("expected a string cell, got {other:?}"),
    }
}

#[test]
fn fields_are_typed() {
    let wb = read_delimited(b"Item,Qty,Flag\nWidget,3,TRUE\nGadget,4.5,false", COMMA).unwrap();
    assert_eq!(text(&wb, 0, 0), "Item");
    assert_eq!(value(&wb, 1, 1), CellValue::Number(3.0));
    assert_eq!(value(&wb, 1, 2), CellValue::Bool(true));
    assert_eq!(value(&wb, 2, 1), CellValue::Number(4.5));
    assert_eq!(value(&wb, 2, 2), CellValue::Bool(false));
}

#[test]
fn quoted_fields_round_trip() {
    // Fields with the delimiter, a quote, and a newline must survive.
    let src = "a,\"b,c\",\"quote\"\"d\"\r\n\"line\nbreak\",e,f\r\n";
    let wb = read_delimited(src.as_bytes(), COMMA).unwrap();
    assert_eq!(text(&wb, 0, 1), "b,c");
    assert_eq!(text(&wb, 0, 2), "quote\"d");
    assert_eq!(text(&wb, 1, 0), "line\nbreak");

    let written = write_delimited(&wb, 0, COMMA);
    let reparsed = read_delimited(written.as_bytes(), COMMA).unwrap();
    assert_eq!(
        wb, reparsed,
        "parse -> write -> parse must be a fixed point"
    );
}

#[test]
fn tsv_and_psv_delimiters() {
    let tsv = read_delimited(b"x\ty\n1\t2", TAB).unwrap();
    assert_eq!(value(&tsv, 1, 0), CellValue::Number(1.0));
    assert_eq!(value(&tsv, 1, 1), CellValue::Number(2.0));

    let psv = read_delimited(b"x|y\n1|2", PIPE).unwrap();
    assert_eq!(value(&psv, 1, 1), CellValue::Number(2.0));
    // A comma inside a pipe-delimited field is ordinary text, not a separator.
    let psv2 = read_delimited(b"a,b|c", PIPE).unwrap();
    assert_eq!(text(&psv2, 0, 0), "a,b");
}

#[test]
fn round_trip_fixed_point_mixed() {
    let src = "Item,Qty,Price\r\nWidget,3,4.5\r\nGadget,5,2\r\n,,\r\n";
    let wb = read_delimited(src.as_bytes(), COMMA).unwrap();
    let written = write_delimited(&wb, 0, COMMA);
    let reparsed = read_delimited(written.as_bytes(), COMMA).unwrap();
    assert_eq!(wb, reparsed);
}

#[test]
fn general_formatting_hides_float_tails() {
    use casual_calc_model::{Cell, Id, Sheet, SheetId};
    let mut wb = casual_calc_model::Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet.cells.set(
        CellRef::new(0, 0),
        Cell::value(CellValue::Number(43.480000000000004)),
    );
    sheet
        .cells
        .set(CellRef::new(0, 1), Cell::value(CellValue::Number(10.0)));
    wb.sheets.push(sheet);
    assert_eq!(write_delimited(&wb, 0, COMMA), "43.48,10\r\n");
}

#[test]
fn empty_input_yields_empty_sheet() {
    let wb = read_delimited(b"", COMMA).unwrap();
    assert!(wb.sheets[0].cells.is_empty());
    assert_eq!(write_delimited(&wb, 0, COMMA), "");
}

#[test]
fn leading_zeros_survive_as_text() {
    let wb = read_delimited(b"007,0042,-0500,0,0.5,0e3", COMMA).unwrap();
    assert_eq!(text(&wb, 0, 0), "007");
    assert_eq!(text(&wb, 0, 1), "0042");
    assert_eq!(text(&wb, 0, 2), "-0500");
    // A bare zero, a fraction and an exponent are ordinary numbers.
    assert_eq!(value(&wb, 0, 3), CellValue::Number(0.0));
    assert_eq!(value(&wb, 0, 4), CellValue::Number(0.5));
    assert_eq!(value(&wb, 0, 5), CellValue::Number(0.0));
}

#[test]
fn zip_code_round_trip_is_a_fixed_point() {
    let wb = read_delimited(b"07030\r\n", COMMA).unwrap();
    let written = write_delimited(&wb, 0, COMMA);
    assert_eq!(written, "07030\r\n");
    assert_eq!(read_delimited(written.as_bytes(), COMMA).unwrap(), wb);
}

#[test]
fn iso_dates_become_serials() {
    let wb = read_delimited(b"2024-03-05,1900-01-01,1900-03-01,1899-12-31", COMMA).unwrap();
    let serial = |c: u32| match value(&wb, 0, c) {
        CellValue::Number(n) => n,
        other => panic!("expected a serial, got {other:?}"),
    };
    assert_eq!(serial(0), 45356.0);
    assert_eq!(serial(1), 1.0);
    // 1900-02-29 does not exist, but Excel counts it, so 1900-03-01 is 61.
    assert_eq!(serial(2), 61.0);
    // Before the epoch there is no serial; it stays text.
    assert_eq!(text(&wb, 0, 3), "1899-12-31");
}

#[test]
fn iso_date_times_and_times_carry_their_format() {
    let wb = read_delimited(
        b"2024-03-05T10:30,2024-03-05 10:30:15,13:45,06:00:30",
        COMMA,
    )
    .unwrap();
    let fmt = |c: u32| {
        let style = wb.sheets[0].cells.get(CellRef::new(0, c)).unwrap().style;
        wb.styles
            .get(style.unwrap())
            .unwrap()
            .number_format
            .clone()
            .unwrap()
    };
    assert_eq!(fmt(0), "yyyy-mm-dd hh:mm");
    assert_eq!(fmt(1), "yyyy-mm-dd hh:mm:ss");
    assert_eq!(fmt(2), "hh:mm");
    assert_eq!(fmt(3), "hh:mm:ss");
    let serial = |c: u32| match value(&wb, 0, c) {
        CellValue::Number(n) => n,
        other => panic!("expected a serial, got {other:?}"),
    };
    assert!((serial(0) - 45356.4375).abs() < 1e-9);
    assert!((serial(2) - 0.572_916_666_666).abs() < 1e-9);
}

#[test]
fn dates_export_as_dates_not_serials() {
    let wb = read_delimited(b"2024-03-05,2024-03-05T10:30,13:45\r\n", COMMA).unwrap();
    let written = write_delimited(&wb, 0, COMMA);
    // The `T` separator normalizes to a space, which is what the number format
    // renders; both forms read back the same, so the model is still a fixed
    // point even though the bytes are not identical.
    assert_eq!(written, "2024-03-05,2024-03-05 10:30,13:45\r\n");
    assert_eq!(read_delimited(written.as_bytes(), COMMA).unwrap(), wb);
}

/// `IO-01`. The reproducer is `fuzz/seeds/delimited/underflow.csv`, inlined
/// rather than included so the assertion can be on the exact CRLF text.
#[test]
fn tiny_numbers_survive_a_save_and_settle() {
    const SRC: &str = "1e-16,1e-300,5e-324,5e-16\r\n-1e-16,-1e-300,-5e-324,-5e-16\r\n";
    let wb = read_delimited(SRC.as_bytes(), COMMA).unwrap();
    let written = write_delimited(&wb, 0, COMMA);

    // On the **text**, not on what this crate's own reader makes of it: a
    // writer and a reader that agree can both be wrong together.
    assert_eq!(written, SRC, "a tiny number was not written as itself");

    // And on the values that come back, bit for bit, so a lost sign counts.
    let reparsed = read_delimited(written.as_bytes(), COMMA).unwrap();
    let expected: [[f64; 4]; 2] = [
        [1e-16, 1e-300, 5e-324, 5e-16],
        [-1e-16, -1e-300, -5e-324, -5e-16],
    ];
    for (r, row) in expected.iter().enumerate() {
        for (c, n) in row.iter().enumerate() {
            assert_eq!(
                number_bits(&reparsed, r as u32, c as u32),
                n.to_bits(),
                "the value at ({r}, {c}) did not survive the save"
            );
        }
    }

    // The second save is a fixed point. `-1e-300` used to be written `-0` and
    // then, on the next save, `0` — the sign vanishing one save after the
    // magnitude did.
    assert_eq!(
        write_delimited(&reparsed, 0, COMMA),
        written,
        "the second save changed the text"
    );
    assert_eq!(reparsed, wb, "the model did not round-trip");
}

/// `IO-01`, the other end: rounding to 15 significant digits must not be able
/// to move a value out of `f64`, and the notation is chosen by magnitude.
#[test]
fn general_notation_follows_magnitude() {
    for (n, expected) in [
        // Positional wherever positional is not longer than the digits it
        // carries — the ordinary case, unchanged.
        (100_000.0, "100000"),
        (0.5, "0.5"),
        (1e-6, "0.000001"),
        (1e20, "100000000000000000000"),
        // Beyond that, the exponent, because 1e300 positional is 301 digits of
        // which 300 are padding.
        (1e-7, "1e-7"),
        (1e-16, "1e-16"),
        (5e-324, "5e-324"),
        (1e21, "1e21"),
        (1e300, "1e300"),
        // Rounding to 15 significant digits rounds `f64::MAX` **up**, past the
        // largest finite double; writing `inf` would be a number leaving as
        // text.
        (f64::MAX, "1.7976931348623157e308"),
        // The tails this rounding exists to hide, still hidden.
        (43.480000000000004, "43.48"),
        (0.1 + 0.2, "0.3"),
    ] {
        assert_eq!(written_number(n), expected, "for {n:e}");
        let back: f64 = expected.parse().expect("the field parses as a number");
        assert_eq!(
            written_number(back),
            expected,
            "the second save of {n:e} moved"
        );
    }
}

/// `IO-02`. Trailing empty fields carry nothing — a row that ends with them
/// reads back identically without them — so padding every row out to the
/// sheet's widest is what turns three cells into a rectangle.
#[test]
fn trailing_blank_fields_are_not_padded_out() {
    // `A1`, `D1`, `A3`: the first row is four fields wide, the third is one.
    let wb = read_delimited(b"a,,,b\r\n\r\nc\r\n", COMMA).unwrap();
    assert_eq!(write_delimited(&wb, 0, COMMA), "a,,,b\r\n\r\nc\r\n");
    // An empty row still costs its line: a row index is an address, so
    // dropping the line would move every cell below it.
    assert_eq!(
        read_delimited(write_delimited(&wb, 0, COMMA).as_bytes(), COMMA).unwrap(),
        wb
    );

    // A cell that exists but renders as nothing — a blank an `.xlsx` kept for
    // its style, which `CellStore::set` stores precisely because it is not
    // blank in the model's sense — is an empty field, and an empty field at
    // the end of a row is the same padding. It cannot hold a column open
    // behind it.
    let mut styled = workbook_of(&[
        (0, 0, CellValue::Number(1.0)),
        (1, 0, CellValue::Number(2.0)),
    ]);
    let style = styled.intern_style(Style {
        number_format: Some("0.00".to_owned()),
        ..Style::default()
    });
    let mut empty = Cell::value(CellValue::Empty);
    empty.style = Some(style);
    styled.sheets[0].cells.set(CellRef::new(0, 5), empty);
    assert_eq!(styled.sheets[0].cells.len(), 3, "the blank cell is stored");
    assert_eq!(write_delimited(&styled, 0, COMMA), "1\r\n2\r\n");
}

/// `IO-02`, the reproducer's shape: `A1`, `(0, n-1)` and `(n, 0)`. The
/// bounding box is `n²` cells; the content is three.
#[test]
fn a_sparse_sheet_saves_in_proportion_to_its_cells() {
    const N: u32 = 2_000;
    let wb = workbook_of(&[
        (0, 0, CellValue::Number(1.0)),
        (0, N - 1, CellValue::Number(2.0)),
        (N, 0, CellValue::Number(3.0)),
    ]);
    let written = write_delimited(&wb, 0, COMMA);

    // Linear in the extent, not quadratic: one delimiter run across the first
    // row, one CRLF per empty row, and the three fields.
    assert!(
        written.len() <= 4 * N as usize,
        "a three-cell sheet wrote {} bytes for an extent of {}",
        written.len(),
        u64::from(N + 1) * u64::from(N)
    );

    // And it is still the same sheet: the addresses are what the delimiters
    // are for.
    let reparsed = read_delimited(written.as_bytes(), COMMA).unwrap();
    assert_eq!(reparsed.sheets[0].cells.len(), 3);
    assert_eq!(value(&reparsed, 0, 0), CellValue::Number(1.0));
    assert_eq!(value(&reparsed, 0, N - 1), CellValue::Number(2.0));
    assert_eq!(value(&reparsed, N, 0), CellValue::Number(3.0));
}

#[test]
fn ambiguous_and_malformed_dates_stay_text() {
    // Locale-ambiguous, out of range, and not-quite-ISO forms all stay text
    // rather than being guessed at.
    let wb = read_delimited(
        b"3/5/2024,2024-13-01,2024-02-30,2024-3-5,2024-03-05x",
        COMMA,
    )
    .unwrap();
    for c in 0..5 {
        assert!(
            matches!(
                value(&wb, 0, c),
                CellValue::SharedString(_) | CellValue::InlineString(_)
            ),
            "column {c} should have stayed text"
        );
    }
}
