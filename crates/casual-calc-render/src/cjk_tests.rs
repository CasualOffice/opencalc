//! CJK through the raster backend (`XL-FEAT-140`). One test, because the slice
//! registry is process-wide and the point is the before and the after.

use casual_calc_layout::{Align, DisplayList, GridGeometry, PaintItem, Rect, Viewport};
use casual_calc_text::cjk::{Region, SliceDirectory, Style, Variant};

use crate::render_pixmap;

fn cell(content: &str, font: &str) -> DisplayList {
    DisplayList {
        items: vec![PaintItem::Text {
            rect: Rect {
                x: 0,
                y: 0,
                w: 3000,
                h: 300,
            },
            content: content.to_owned(),
            align: Align::Left,
            color: None,
            bold: false,
            italic: false,
            font_name: Some(font.to_owned()),
            font_pt: Some(11.0),
        }],
    }
}

/// Dark pixels in the cell — glyph ink.
fn ink(list: &DisplayList) -> usize {
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 3000,
        height: 300,
    };
    let pixmap = render_pixmap(list, &GridGeometry::default(), &viewport, 96).unwrap();
    pixmap
        .pixels()
        .iter()
        .filter(|p| p.red() < 128 && p.green() < 128 && p.blue() < 128)
        .count()
}

/// Untrusted text cannot make the report unbounded: a cell of seventy
/// thousand distinct characters no face draws is reported as a capped list and
/// an exact count, and the export still finishes.
#[test]
fn a_flood_of_undrawable_characters_is_counted_not_listed_without_bound() {
    use crate::pdf::{MAX_REPORTED_CHARS, PdfMetadata, PdfPage, write_pdf};
    // CJK Extension B, which none of this binary's tests load a slice for.
    let flood: String = (0x2_0000u32..0x2_0000 + 70_000)
        .filter_map(char::from_u32)
        .collect();
    let list = cell(&flood, "Calibri");
    let page = PdfPage {
        width: 12240,
        height: 15840,
        margin_left: 1008,
        margin_top: 1080,
        scale: 1.0,
        bands: vec![crate::pdf::PdfBand {
            display_list: &list,
            rows: (0, 0),
            cols: (0, 0),
            origin: (0, 0),
            gridlines: false,
        }],
        header: Default::default(),
        footer: Default::default(),
    };
    let (pdf, report) =
        write_pdf(&[page], &GridGeometry::default(), &PdfMetadata::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(report.glyphs.undrawn.len(), MAX_REPORTED_CHARS);
    assert_eq!(report.glyphs.undrawn_total, 70_000);
    let wanted: usize = report.glyphs.wanted.values().map(|s| s.len()).sum();
    assert!(wanted <= MAX_REPORTED_CHARS, "{wanted}");
}

#[test]
fn a_png_draws_japanese_once_its_slices_are_supplied_and_nothing_before() {
    let text = "日本語のテキスト";
    let before = ink(&cell(text, "游ゴシック"));
    assert_eq!(before, 0, "no CJK face loaded, so nothing to draw");

    let dir = SliceDirectory::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../webapp/fonts/cjk"),
    )
    .unwrap();
    let jp = Variant::new(Region::Jp, Style::Sans, false);
    assert!(dir.load(text.chars().map(|c| (jp, c))) > 0);

    let after = ink(&cell(text, "游ゴシック"));
    assert!(after > 200, "{after} dark pixels for eight characters");

    // Latin-font fallback (font linking): the same text in Calibri draws the
    // same glyphs, because kana makes the run Japanese.
    // Not pixel-equal: the cell's baseline comes from its Latin face (Carlito
    // here, Liberation Sans for 游ゴシック), so the anti-aliasing lands
    // differently — the ink is the same to within a few percent.
    let linked = ink(&cell(text, "Calibri"));
    assert!(
        linked.abs_diff(after) * 20 < after,
        "Calibri falls back to the same Japanese face: {linked} against {after}"
    );
}
