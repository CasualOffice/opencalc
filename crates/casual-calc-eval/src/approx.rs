//! Number equality the way a spreadsheet means it (`XL-FEAT-011`).
//!
//! `0.1 + 0.2` is `0.30000000000000004` in binary floating point, one unit in
//! the last place from the double nearest `0.3`. Excel answers `=0.1+0.2=0.3`
//! with TRUE: it carries 15 significant digits ("Floating-point arithmetic may
//! give inaccurate results in Excel", Microsoft), and a difference in the bits
//! beyond them is not a difference a user can see or mean. Comparing the raw
//! doubles made every `IF(total=budget, …)` over computed money a coin toss.
//!
//! Microsoft does not publish the exact tolerance. The rule here is
//! LibreOffice's `rtl::math::approxEqual`, the one open implementation built
//! to agree with Excel on this: two numbers are equal when they differ by less
//! than 2⁻⁴⁸ of each — about 3.6 parts in 10¹⁵, so a difference in the 15th
//! significant digit still counts — **except** that two whole numbers whose
//! difference is a whole number compare exactly. Without that exception the
//! relative tolerance would call `999999999999999` and `999999999999998` equal,
//! and two adjacent fifteen-digit account numbers are not the same account.
//!
//! The rule is for *comparison*. Sorting stays on the exact order, because a
//! tolerance is not transitive (`a≈b`, `b≈c`, `a≉c`) and a sort over a
//! non-transitive order has no defined answer.

use std::cmp::Ordering;

/// 2⁻⁴⁸.
const E48: f64 = 1.0 / 281_474_976_710_656.0;
/// 2⁵³: the last magnitude at which every whole number has a double.
const TWO_53: f64 = 9_007_199_254_740_992.0;

fn representable_integer(x: f64) -> bool {
    x.abs() <= TWO_53 && x.fract() == 0.0
}

/// Whether `a` and `b` are the same number to a spreadsheet.
pub(crate) fn approx_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    // Zero is only ever equal to zero: no ratio of zero is a tolerance.
    if a == 0.0 || b == 0.0 {
        return false;
    }
    let d = (a - b).abs();
    if !d.is_finite() {
        return false;
    }
    let (a, b) = (a.abs(), b.abs());
    if representable_integer(d) && representable_integer(a) && representable_integer(b) {
        return false;
    }
    d < a * E48 && d < b * E48
}

/// `a` against `b`, equal within [`approx_eq`]; `None` only for a NaN.
pub(crate) fn approx_cmp(a: f64, b: f64) -> Option<Ordering> {
    if approx_eq(a, b) {
        Some(Ordering::Equal)
    } else {
        a.partial_cmp(&b)
    }
}

/// `a + b`, or exactly zero when the two cancel to within [`approx_eq`].
///
/// Excel 97 and later: "Should an addition or subtraction operation result in
/// a value at or very close to zero, Excel 97 and later will compensate for any
/// error introduced as a result of converting an operand to and from binary"
/// (Microsoft, same article). `=1.333+1.225-1.333-1.225` is 0, not -2.2E-16.
pub(crate) fn snapped_add(a: f64, b: f64) -> f64 {
    if (a < 0.0) != (b < 0.0) && approx_eq(a, -b) {
        0.0
    } else {
        a + b
    }
}

/// `a - b`, or exactly zero when the two are equal within [`approx_eq`].
pub(crate) fn snapped_sub(a: f64, b: f64) -> f64 {
    if approx_eq(a, b) { 0.0 } else { a - b }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tolerance_is_the_last_bits_only() {
        assert!(approx_eq(0.1 + 0.2, 0.3));
        assert!(approx_eq(-(0.1 + 0.2), -0.3));
        assert!(!approx_eq(1.0, 1.0 + 1e-10));
        assert!(!approx_eq(0.0, 1e-300));
        assert!(!approx_eq(f64::MAX, -f64::MAX));
        assert!(!approx_eq(999_999_999_999_999.0, 999_999_999_999_998.0));
        assert_eq!(approx_cmp(0.1 + 0.2, 0.3), Some(Ordering::Equal));
        assert_eq!(approx_cmp(1.0, 2.0), Some(Ordering::Less));
    }

    #[test]
    fn snapping_needs_cancellation() {
        assert_eq!(snapped_sub(0.5 - 0.4, 0.1), 0.0);
        assert_eq!(snapped_add(0.1 + 0.2, -0.3), 0.0);
        // Same sign: nothing cancels, nothing snaps.
        assert_eq!(snapped_add(0.1, 0.2), 0.1 + 0.2);
        assert_eq!(snapped_sub(1.0, 0.999), 1.0 - 0.999);
    }
}
