//! Japanese, Chinese and Korean faces for the editor and its exports
//! (`XL-FEAT-140`).
//!
//! The faces are **not in this binary**. They are WOFF2 slices under
//! `fonts/cjk/`, served as static files; the browser fetches the ones the
//! canvas draws through `@font-face` `unicode-range`, and before a PDF export
//! the editor asks [`session_export_pdf_font_needs`] which slices the page
//! wants, fetches those same files and hands them to [`register_font_slice`].

use super::*;
use casual_calc_render::cjk::{Region, Variant, region_for, set_default_region};

/// The CSS font stack for `name` when the cell's text belongs to `region`
/// (`jp`, `sc`, `tc`, `kr`): the Latin substitute, the requested name, then
/// the four CJK faces with `region` first. An unknown region code uses the
/// default region.
#[wasm_bindgen]
pub fn font_css_stack_for(name: &str, region: &str) -> String {
    let region = Region::parse(region).unwrap_or_else(casual_calc_render::cjk::default_region);
    casual_calc_layout::css_stack_for(name, region)
}

/// The region (`jp`, `sc`, `tc`, `kr`) `text` in a cell styled `family` is
/// drawn in: the font's own when it is an East Asian font, else kana → `jp`,
/// hangul → `kr`, bopomofo → `tc`, else the default region. The same rule the
/// exporters use, so the screen and the PDF draw a Han character in the same
/// regional form.
#[wasm_bindgen]
#[must_use]
pub fn cjk_region_for(family: &str, text: &str) -> String {
    let family = (!family.is_empty()).then_some(family);
    region_for(family, text).code().to_owned()
}

/// Set the region Han characters in a Latin-font cell default to — Excel's
/// equivalent is the system locale. Accepts `jp`/`sc`/`tc`/`kr` or a language
/// tag (`ja`, `zh-CN`, `zh-TW`, `ko`). Returns whether it was understood; an
/// unknown value leaves the default as it was.
#[wasm_bindgen]
#[must_use]
pub fn set_cjk_default_region(code: &str) -> bool {
    Region::parse(code).map(set_default_region).is_some()
}

/// Register one CJK font slice, as fetched: `variant` is its directory
/// (`jp-sans-400`), `bytes` the WOFF2 file. Returns how many characters it
/// maps.
///
/// # Errors
///
/// An unknown variant, bytes that are not a readable face (a 404 page, a
/// truncated download), or a registry past its size cap — each refused rather
/// than stored.
#[wasm_bindgen]
pub fn register_font_slice(variant: &str, bytes: &[u8]) -> Result<u32, JsError> {
    let variant =
        Variant::from_key(variant).ok_or_else(|| JsError::new("unknown CJK font variant"))?;
    let chars = casual_calc_render::cjk::register_slice(variant, bytes).map_err(js)?;
    Ok(u32::try_from(chars).unwrap_or(u32::MAX))
}

/// The CJK slices a PDF of `sheet_index` wants and does not have, as JSON
/// `[{"chain":["jp-sans-400","sc-sans-400",…],"chars":"日本語"}]`.
///
/// `chain` is where to look for each character, in order — the variant meant
/// for it, then the same style and weight in the other regions — exactly the
/// walk the canvas's CSS stack makes. A host fetches, for each character, the
/// slice of the first variant in the chain whose ranges hold it, registers
/// them, and asks again; an empty array means the export will draw every
/// character in the face meant for it. Empty, too, when there is no session.
#[wasm_bindgen]
#[must_use]
pub fn session_export_pdf_font_needs(sheet_index: usize) -> String {
    let needs =
        with_session(|s| s.pdf_font_needs(sheet_index).unwrap_or_default()).unwrap_or_default();
    let items: Vec<String> = needs
        .iter()
        .map(|(variant, chars)| {
            let chain: Vec<String> = variant
                .chain()
                .iter()
                .map(|v| json_string(&v.key()))
                .collect();
            format!(
                "{{\"chain\":[{}],\"chars\":{}}}",
                chain.join(","),
                json_string(chars)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}
