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

/// `XL-FEAT-140`: an East Asian font name draws with the mapped CJK face on
/// screen. The editor's canvas uses this stack, so a cell styled `游ゴシック`
/// that resolves to a Latin face draws its Japanese in whatever the viewer
/// happens to have installed — or nothing.
#[test]
fn east_asian_font_names_lead_their_stack_with_the_mapped_cjk_face() {
    use crate::substitution::css_stack;
    for (name, face) in [
        ("游ゴシック", "OpenCalc CJK JP Sans"),
        ("Yu Gothic", "OpenCalc CJK JP Sans"),
        ("ＭＳ Ｐゴシック", "OpenCalc CJK JP Sans"),
        ("Meiryo", "OpenCalc CJK JP Sans"),
        ("ＭＳ 明朝", "OpenCalc CJK JP Serif"),
        ("Yu Mincho", "OpenCalc CJK JP Serif"),
        ("Microsoft YaHei", "OpenCalc CJK SC Sans"),
        ("等线", "OpenCalc CJK SC Sans"),
        ("SimSun", "OpenCalc CJK SC Serif"),
        ("Microsoft JhengHei", "OpenCalc CJK TC Sans"),
        ("PMingLiU", "OpenCalc CJK TC Serif"),
        ("맑은 고딕", "OpenCalc CJK KR Sans"),
        ("Batang", "OpenCalc CJK KR Serif"),
    ] {
        let stack = css_stack(name);
        assert!(
            stack.starts_with(&format!("\"{face}\"")),
            "{name}: expected the stack to lead with {face}, got {stack}"
        );
        assert!(
            stack.contains(&format!("\"{name}\"")),
            "{name}: the requested name stays in the stack: {stack}"
        );
    }
}

/// And a Latin font falls back to the CJK faces for the characters it lacks —
/// Excel's font linking — rather than to the viewer's system font.
#[test]
fn a_latin_font_stack_falls_back_to_the_cjk_faces() {
    let stack = crate::substitution::css_stack("Calibri");
    assert!(stack.starts_with("\"Carlito\", \"Calibri\""), "{stack}");
    for face in [
        "OpenCalc CJK JP Sans",
        "OpenCalc CJK SC Sans",
        "OpenCalc CJK TC Sans",
        "OpenCalc CJK KR Sans",
    ] {
        assert!(stack.contains(face), "{face} missing from {stack}");
    }
}

/// CJK text is measured at its real width — one em per ideograph, kana or
/// hangul syllable — even before any CJK face has been loaded. Measured at zero
/// a column of Japanese autofits to nothing and overflow never triggers.
#[test]
fn cjk_text_measures_one_em_per_character_before_its_face_loads() {
    // Hangul is narrower than Han in the Korean faces: 0.92 em in the sans.
    for (text, family, em) in [
        ("日本語", Some("Calibri"), 1.0),
        ("简体字", Some("Arial"), 1.0),
        ("한국어", None, 0.92),
        ("ひらがな", Some("游ゴシック"), 1.0),
    ] {
        let n = text.chars().count() as f32;
        let got = advance_width(text, 16.0, false, false, family);
        assert!(
            (got - 16.0 * n * em).abs() < 0.01,
            "{text} in {family:?}: {got}px, expected {}",
            16.0 * n * em
        );
    }
}
