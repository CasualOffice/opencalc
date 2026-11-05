//! Excel's units for column width and row height, in one place.
//!
//! OOXML measures a column in *characters* of the default font's maximum digit
//! width and a row in *points*; the model keeps both in twips. These
//! conversions used to live in two crates — the reader's half in
//! `casual-calc-import` and the writer's inverse in `casual-calc-export` — and
//! the editor's Column Width and Row Height dialogs need both halves too
//! (`XL-UX-035`). A third copy in JavaScript is how a dialog comes to say 8.43
//! for a column the file calls 8.42, so the dialogs reach these through the
//! wasm crate instead. They live in the model because it is the one crate the
//! reader, the writer and the wasm bindings all depend on.
//!
//! The constants are Calibri 11's at 96 dpi, which is what Excel's default
//! workbook and this engine's default grid both use: a maximum digit width of
//! 7 px and 5 px of cell padding, so the default 64 px column is
//! `(64 - 5) / 7 = 8.43` characters and the default 20 px row is 15 points.

/// Ceiling on a column width, in twips: Excel refuses a column wider than 255
/// characters, which is `255 * 7 + 5 = 1790` px, or 26_850 twips.
pub const MAX_COL_TWIPS: i64 = 26_850;

/// Ceiling on a row height, in twips: Excel refuses a row taller than 409.5
/// points, or 8_190 twips.
pub const MAX_ROW_TWIPS: i64 = 8_190;

/// The widest column Excel accepts, in characters.
pub const MAX_COL_CHARS: f64 = 255.0;

/// The tallest row Excel accepts, in points.
pub const MAX_ROW_POINTS: f64 = 409.5;

/// Convert an Excel column width (character units) to twips, matching the
/// pixel rounding Excel uses so the value survives a write→read round-trip.
///
/// `chars` is an arbitrary `xsd:double` off the wire — `<col width>` and
/// `<sheetFormatPr defaultColWidth>` are attacker-controlled — and nothing used
/// to bound it. `width="1e300"` saturated the float→int cast to `i64::MAX` and
/// the `* 15` panicked with "attempt to multiply with overflow" under the dev
/// profile, so a crafted `<cols>` aborted the host process instead of returning
/// `Err`; the shipped release profile wrapped instead, and `width="-1e300"`
/// handed layout a *negative* column width. Clamp before the multiply, not
/// after — after is already too late.
#[must_use]
pub fn col_width_to_twips(chars: f64) -> i64 {
    let px = (chars * 7.0 + 5.0).round();
    // Two layers, and they answer different questions. `read_f64_attr` refuses
    // a non-finite attribute at the boundary, so nothing from a file arrives
    // here as `NaN`; this function takes a plain `f64` from anywhere (the
    // editor's Column Width dialog among them), so it still declines to trust
    // one. `NaN` is named rather than left to fall through a comparison it
    // silently fails — it is neither `<` nor `>`, so relying on that would send
    // it to the ceiling below.
    if !px.is_finite() || px <= 0.0 {
        return 0;
    }
    let max_px = (MAX_COL_TWIPS / 15) as f64;
    if px > max_px {
        return MAX_COL_TWIPS;
    }
    (px as i64) * 15
}

/// Convert an Excel row height (points) to twips. Bounded for the same reason
/// as [`col_width_to_twips`]: `ht="1e308"` saturated the cast and yielded
/// `i64::MAX` twips of row.
#[must_use]
pub fn row_height_to_twips(points: f64) -> i64 {
    let twips = (points * 20.0).round();
    if !twips.is_finite() || twips <= 0.0 {
        return 0;
    }
    if twips > MAX_ROW_TWIPS as f64 {
        return MAX_ROW_TWIPS;
    }
    twips as i64
}

/// Reverse of [`col_width_to_twips`]: twips → Excel character width. Chosen so
/// `read(write(x)) == x` for import-derived widths. Unrounded — this is what
/// the writer puts in `<col width>`.
#[must_use]
pub fn twips_to_col_chars(twips: i64) -> f64 {
    ((twips as f64 / 15.0) - 5.0) / 7.0
}

/// Reverse of [`row_height_to_twips`]: twips → points.
#[must_use]
pub fn twips_to_row_points(twips: i64) -> f64 {
    twips as f64 / 20.0
}

/// A column width as Excel's Column Width dialog shows it: characters, to two
/// decimals, by ECMA-376's own rounding,
/// `Truncate([{pixels} - 5] / {maxDigitWidth} * 100 + 0.5) / 100`.
///
/// Never negative: a column narrower than its padding reads as 0.
#[must_use]
pub fn col_chars_for_display(twips: i64) -> f64 {
    let chars = twips_to_col_chars(twips);
    ((chars * 100.0 + 0.5).trunc() / 100.0).max(0.0)
}

/// A row height as Excel's Row Height dialog shows it: points, to two decimals.
#[must_use]
pub fn row_points_for_display(twips: i64) -> f64 {
    (twips_to_row_points(twips) * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults both ways: Excel's 8.43 characters is this grid's 64 px
    /// column, and 15 points is its 20 px row.
    #[test]
    fn defaults_round_trip_through_the_dialog_units() {
        assert_eq!(col_width_to_twips(8.43), 64 * 15);
        assert_eq!(col_chars_for_display(64 * 15), 8.43);
        assert_eq!(row_height_to_twips(15.0), 20 * 15);
        assert_eq!(row_points_for_display(20 * 15), 15.0);
    }

    #[test]
    fn display_rounding_is_ecma_376s() {
        // 100 px: (100 - 5) / 7 = 13.571… → 13.57.
        assert_eq!(col_chars_for_display(100 * 15), 13.57);
        // A column below its padding is 0, not negative.
        assert_eq!(col_chars_for_display(3 * 15), 0.0);
        assert_eq!(col_width_to_twips(MAX_COL_CHARS), MAX_COL_TWIPS);
        assert_eq!(row_height_to_twips(MAX_ROW_POINTS), MAX_ROW_TWIPS);
    }
}
