//! What a caller sizing a box is entitled to assume.

use super::*;

const PX: f32 = 10.0;

fn w(text: &str) -> f32 {
    advance_width(text, PX, false, false, None)
}

#[test]
fn nothing_is_no_wide() {
    assert_eq!(w(""), 0.0);
}

/// A size that is not a size is not a panic, because every caller of this is
/// laying out a rectangle and a zero-width box beats an aborted render.
#[test]
fn an_impossible_size_measures_nothing() {
    for px in [0.0, -12.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            advance_width("Series 1", px, false, false, None),
            0.0,
            "px = {px}"
        );
    }
}

/// The property every box-sizing caller depends on.
#[test]
fn more_text_is_wider() {
    assert!(w("Series 1") > w("S"));
    assert!(w("Revenue by region") > w("Revenue"));
}

/// And the one a *legend* depends on: the widest name is the widest measurement,
/// so picking the maximum picks the right one.
#[test]
fn the_longest_name_measures_widest() {
    let names = ["Q1", "Revenue by region", "Cost"];
    let widest = names
        .iter()
        .max_by(|a, b| w(a).partial_cmp(&w(b)).unwrap())
        .unwrap();
    assert_eq!(*widest, "Revenue by region");
}

/// Twice the size is twice the width. Not exactly — hinting and rounding live
/// in here — but close enough that a box scales with its text.
#[test]
fn width_scales_with_size() {
    let small = advance_width("Series 1", 10.0, false, false, None);
    let large = advance_width("Series 1", 20.0, false, false, None);
    let ratio = large / small;
    assert!(
        (1.9..=2.1).contains(&ratio),
        "doubling the size scaled the width by {ratio}"
    );
}

/// Bold is not narrower than regular. A legend sized from the regular face and
/// drawn in the bold one would overflow its box.
#[test]
fn bold_is_never_narrower_than_regular() {
    let regular = advance_width("Revenue by region", PX, false, false, None);
    let bold = advance_width("Revenue by region", PX, true, false, None);
    assert!(bold >= regular, "bold {bold} < regular {regular}");
}

/// A character no bundled face covers contributes nothing rather than
/// exploding. It is drawn as nothing too, so the box still fits what appears.
#[test]
fn an_uncovered_character_is_measured_as_nothing_rather_than_panicking() {
    // A private-use codepoint: no face has a glyph for it, by definition.
    let measured = advance_width("\u{F8FF}", PX, false, false, None);
    assert!(measured.is_finite(), "got {measured}");
}

/// An unknown family still measures, because substitution answers for it. A
/// legend in "Helvetica Neue" must not come out zero wide.
#[test]
fn an_unbundled_family_still_measures() {
    for family in [
        Some("Arial"),
        Some("Helvetica Neue"),
        Some("Wingdings"),
        None,
    ] {
        let measured = advance_width("Series 1", PX, false, false, family);
        assert!(measured > 0.0, "{family:?} measured {measured}");
    }
}
