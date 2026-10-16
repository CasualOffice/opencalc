//! The CJK faces against the slices this repository ships (`XL-FEAT-140`).
//!
//! The registry is process-wide, so every test here only *adds* slices, and a
//! test that asserts something about the moment before a slice loads uses a
//! variant no other test in this binary registers.

use crate::cjk::{Region, SliceDirectory, Style, Variant, loaded_face, nominal_advance_em};
use crate::faces::TextFaces;
use crate::{advance_width, woff2};
use skrifa::{FontRef, MetadataProvider, instance::LocationRef, prelude::Size};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../webapp/fonts/cjk")
}

fn dir() -> SliceDirectory {
    SliceDirectory::open(root()).expect("the shipped slice directory")
}

fn wanted(variant: Variant, text: &str) -> Vec<(Variant, char)> {
    text.chars().map(|c| (variant, c)).collect()
}

const JP_SANS: Variant = Variant {
    region: Region::Jp,
    style: Style::Sans,
    weight: 400,
};

#[test]
fn a_loaded_slice_draws_and_measures_its_characters() {
    let dir = dir();
    assert!(dir.load(wanted(JP_SANS, "日本語")) >= 1, "a slice was read");
    let faces = TextFaces::new(Some("Calibri"), false, false, "日本語");
    for ch in "日本語".chars() {
        let r = faces.resolve(ch);
        assert_eq!(r.face, loaded_face(JP_SANS, ch), "{ch} from the JP slice");
        assert!(r.face.is_some(), "{ch} has a face");
        assert_eq!(r.wanted, None, "{ch} needs nothing more");
    }
    let w = advance_width("日本語", 16.0, false, false, Some("Calibri"));
    assert!((w - 48.0).abs() < 0.01, "{w}");
}

#[test]
fn kana_in_a_latin_cell_draws_its_han_in_the_japanese_face_and_hangul_in_the_korean() {
    let dir = dir();
    let kr = Variant::new(Region::Kr, Style::Sans, false);
    dir.load(wanted(JP_SANS, "東京と"));
    dir.load(wanted(kr, "서울東"));
    let jp_cell = TextFaces::new(Some("Calibri"), false, false, "東京と");
    let kr_cell = TextFaces::new(Some("Calibri"), false, false, "서울東");
    assert_eq!(jp_cell.resolve('東').face, loaded_face(JP_SANS, '東'));
    assert_eq!(kr_cell.resolve('東').face, loaded_face(kr, '東'));
    assert_ne!(
        loaded_face(JP_SANS, '東'),
        loaded_face(kr, '東'),
        "two regions, two faces for the same Han character"
    );
}

#[test]
fn a_character_its_region_lacks_comes_from_the_next_region_in_the_chain() {
    let dir = dir();
    let index = dir.index();
    // A Han character the Japanese slices do not have and the Chinese do.
    let ch = (0x4E00u32..0x9FFF)
        .filter_map(char::from_u32)
        .find(|&c| {
            index.slice_for(Region::Jp, c).is_none() && index.slice_for(Region::Sc, c).is_some()
        })
        .expect("Simplified Chinese has characters JIS does not");
    let jp_bold = Variant::new(Region::Jp, Style::Sans, true);
    let (hit, _) = index.locate(jp_bold, ch).unwrap();
    assert_ne!(hit.region, Region::Jp);
    dir.load([(jp_bold, ch)]);
    let r = TextFaces::new(Some("Calibri"), true, false, &format!("と{ch}")).resolve(ch);
    assert!(r.face.is_some(), "{ch} is drawn from {:?}", hit.region);
    assert_eq!(r.face, loaded_face(hit, ch));
    assert_eq!(
        r.wanted,
        Some(jp_bold),
        "drawn by a stand-in region, so still named"
    );
}

#[test]
fn an_east_asian_font_draws_its_own_latin_once_its_slice_loads() {
    // Korean serif: no other test in this binary registers it.
    let kr_serif = Variant::new(Region::Kr, Style::Serif, false);
    let faces = TextFaces::new(Some("바탕"), false, false, "Batang Q");
    let before = faces.resolve('Q');
    assert!(before.face.is_some(), "Latin stand-in meanwhile");
    assert_eq!(before.wanted, Some(kr_serif), "the slice is wanted");
    assert!(dir().load([(kr_serif, 'Q')]) >= 1);
    let after = faces.resolve('Q');
    assert_eq!(after.face, loaded_face(kr_serif, 'Q'));
    assert_eq!(after.wanted, None);
}

#[test]
fn a_bold_serif_cell_is_drawn_emboldened_because_no_serif_bold_ships() {
    let tc_serif = Variant::new(Region::Tc, Style::Serif, true);
    assert_eq!(tc_serif.weight, 400);
    dir().load([(tc_serif, '明')]);
    let r = TextFaces::new(Some("PMingLiU"), true, false, "明").resolve('明');
    assert!(r.face.is_some());
    assert!(r.synthetic_bold, "Excel emboldens PMingLiU; so do we");
    let sans = TextFaces::new(Some("Calibri"), false, false, "明").resolve('明');
    assert!(!sans.synthetic_bold);
}

/// The width measured before a slice loads is the width it has after.
///
/// Every character [`nominal_advance_em`] claims a width for, in every slice
/// of three regions, has exactly that advance — so a column autofitted before
/// its font arrives is not wrong for its CJK text.
#[test]
fn the_nominal_width_is_the_real_width_in_every_shipped_slice() {
    let mut checked = 0usize;
    // Every region and both styles; the bold sans shares its widths with the
    // regular (an instance of one variable font), so one weight stands for it.
    for (key, style) in [
        ("jp-sans-400", Style::Sans),
        ("sc-serif-400", Style::Serif),
        ("tc-sans-700", Style::Sans),
        ("kr-sans-400", Style::Sans),
        ("kr-serif-400", Style::Serif),
    ] {
        checked += nominal_matches(key, style);
    }
    assert!(checked > 70_000, "checked {checked} characters");
}

fn nominal_matches(key: &str, style: Style) -> usize {
    let mut checked = 0usize;
    {
        let vdir = root().join(key);
        let mut files: Vec<_> = std::fs::read_dir(&vdir)
            .unwrap_or_else(|e| panic!("{}: {e}", vdir.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        for path in files {
            let sfnt = woff2::to_sfnt(&std::fs::read(&path).unwrap(), 1 << 24).unwrap();
            let font = FontRef::new(&sfnt).unwrap();
            let upem = f32::from(font.head().unwrap().units_per_em());
            let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
            for (cp, gid) in font.charmap().mappings() {
                let Some(ch) = char::from_u32(cp) else {
                    continue;
                };
                let Some(em) = nominal_advance_em(ch, style) else {
                    continue;
                };
                let got = metrics.advance_width(gid).unwrap_or(0.0) / upem;
                assert!(
                    (got - em).abs() < 1e-3,
                    "{key} {}: U+{cp:04X} is {got} em, nominal says {em}",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    checked
}

use skrifa::raw::TableProvider as _;
