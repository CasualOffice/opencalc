//! Japanese, Chinese and Korean through the PDF export (`XL-FEAT-140`).
//!
//! One test, in order, because the slice registry is process-wide: what the
//! export says **before** the host supplies any slice, then the host's loop
//! (ask, load, export), then what the file carries.

use std::collections::BTreeMap;

use casual_calc_render::cjk::{SliceDirectory, Variant};

use crate::WorkbookSession;

/// The fixture: one row per region and style, in the font Excel defaults to
/// for that language, plus the Calibri font-linking cases.
const ROWS: [&str; 12] = [
    "日本語のテキスト",
    "明朝体で書かれた文章",
    "简体中文文本",
    "宋体的中文",
    "繁體中文文字",
    "新細明體的字",
    "한국어 텍스트",
    "바탕체 글자",
    "Calibri と日本語",
    "Calibri 한국어",
    "Latin ABC 太字",
    "太字の明朝",
];

fn fixture() -> WorkbookSession {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/generated/cjk-east-asian.xlsx");
    WorkbookSession::open(std::fs::read(path).expect("the fixture")).expect("opens")
}

fn slices() -> SliceDirectory {
    SliceDirectory::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../webapp/fonts/cjk"),
    )
    .expect("the shipped slices")
}

/// The objects of a PDF this crate wrote, by number.
fn objects(pdf: &[u8]) -> BTreeMap<u32, &[u8]> {
    let mut out = BTreeMap::new();
    let mut at = 0;
    while let Some(found) = find(&pdf[at..], b" 0 obj\n") {
        let end_of_marker = at + found + b" 0 obj\n".len();
        let line_start = pdf[..at + found]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let id: u32 = std::str::from_utf8(&pdf[line_start..at + found])
            .unwrap()
            .parse()
            .unwrap();
        let close = find(&pdf[end_of_marker..], b"\nendobj").unwrap() + end_of_marker;
        out.insert(id, &pdf[end_of_marker..close]);
        at = close;
    }
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn stream(obj: &[u8]) -> &[u8] {
    let s = find(obj, b"stream\n").unwrap() + 7;
    let e = obj.len() - b"\nendstream".len();
    &obj[s..e]
}

fn reference(dict: &str, key: &str) -> Option<u32> {
    let at = dict.find(key)? + key.len();
    dict[at..].split_whitespace().next()?.parse().ok()
}

/// The text the PDF shows, recovered **per font** through each font's own
/// `ToUnicode` map — as a viewer's copy and search do. Several CJK slices on
/// one page reuse the same glyph ids, so one map merged across fonts would
/// read the wrong characters; this follows `Tf` the way a viewer does.
fn extract(pdf: &[u8]) -> String {
    let objs = objects(pdf);
    let mut fonts: BTreeMap<String, BTreeMap<u32, String>> = BTreeMap::new();
    let mut contents = Vec::new();
    for body in objs.values() {
        let text = String::from_utf8_lossy(body);
        if text.starts_with("<< /ExtGState") {
            let at = text.find("/Font <<").unwrap() + 8;
            let mut tokens = text[at..].split_whitespace();
            while let Some(name) = tokens.next() {
                if name == ">>" {
                    break;
                }
                let id: u32 = tokens.next().unwrap().parse().unwrap();
                tokens.next();
                tokens.next();
                let type0 = String::from_utf8_lossy(objs[&id]);
                let cmap_id = reference(&type0, "/ToUnicode ").unwrap();
                let cmap = String::from_utf8_lossy(stream(objs[&cmap_id])).into_owned();
                let mut map = BTreeMap::new();
                for line in cmap.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() == 2 && parts[0].starts_with('<') && parts[1].starts_with('<') {
                        let gid =
                            u32::from_str_radix(parts[0].trim_matches(['<', '>']), 16).unwrap();
                        let hex = parts[1].trim_matches(['<', '>']);
                        let units: Vec<u16> = (0..hex.len() / 4)
                            .map(|i| u16::from_str_radix(&hex[i * 4..i * 4 + 4], 16).unwrap())
                            .collect();
                        map.insert(gid, String::from_utf16(&units).unwrap());
                    }
                }
                fonts.insert(name.trim_start_matches('/').to_owned(), map);
            }
        } else if text.contains(" cm") && text.contains("stream\n") {
            contents.push(String::from_utf8_lossy(stream(body)).into_owned());
        }
    }
    let mut out = String::new();
    for content in contents {
        let mut font = String::new();
        let tokens: Vec<&str> = content.split_whitespace().collect();
        for (i, token) in tokens.iter().enumerate() {
            if *token == "Tf" {
                font = tokens[i - 2].trim_start_matches('/').to_owned();
            } else if *token == "Tm" {
                out.push(' ');
            }
            let mut rest = *token;
            while let Some(open) = rest.find('<') {
                let close = rest[open..].find('>').map(|c| c + open);
                let Some(close) = close else { break };
                let hex = &rest[open + 1..close];
                if !hex.is_empty() && hex.len() % 4 == 0 {
                    for k in 0..hex.len() / 4 {
                        let gid = u32::from_str_radix(&hex[k * 4..k * 4 + 4], 16).unwrap();
                        out.push_str(
                            fonts
                                .get(&font)
                                .and_then(|m| m.get(&gid))
                                .map_or("\u{fffd}", String::as_str),
                        );
                    }
                }
                rest = &rest[close + 1..];
            }
        }
    }
    out
}

fn embedded_font_bytes(pdf: &[u8]) -> usize {
    objects(pdf)
        .values()
        .filter_map(|o| {
            let t = String::from_utf8_lossy(o);
            t.starts_with("<< /Length")
                .then(|| reference(&t, "/Length1 "))
                .flatten()
        })
        .map(|n| n as usize)
        .sum()
}

#[test]
fn japanese_chinese_and_korean_export_draw_and_extract_as_the_same_text() {
    let session = fixture();

    // Before the host supplies anything: every CJK character is named, both as
    // what to fetch and — because the page would otherwise silently show
    // empty cells — in the export's report.
    let needs = session.pdf_font_needs(0).unwrap();
    let wanted: BTreeMap<String, String> = needs
        .iter()
        .map(|(v, chars)| (v.key(), chars.clone()))
        .collect();
    for key in [
        "jp-sans-400",
        "jp-serif-400",
        "sc-sans-400",
        "sc-serif-400",
        "tc-sans-400",
        "tc-serif-400",
        "kr-sans-400",
        "kr-serif-400",
        "jp-sans-700",
    ] {
        assert!(wanted.contains_key(key), "{key} not asked for: {wanted:?}");
    }
    assert!(
        wanted["jp-sans-700"].contains('L'),
        "the bold ＭＳ Ｐゴシック cell wants its own Latin: {wanted:?}"
    );
    let (_, before) = session.export_pdf_with_report(0).unwrap();
    let named: Vec<String> = before.entries().iter().map(|e| e.feature.clone()).collect();
    assert!(
        named.iter().any(|f| f.contains("CJK") && f.contains('日')),
        "undrawn Han is named with a sample: {named:?}"
    );
    assert!(named.iter().any(|f| f.contains("Korean")), "{named:?}");

    // The host's loop: ask, load, ask again — one round suffices, and a second
    // finds nothing more to fetch.
    let dir = slices();
    let wants = needs
        .iter()
        .flat_map(|(v, chars)| chars.chars().map(move |c| (*v, c)));
    let loaded = dir.load(wants);
    assert!(
        loaded > 0 && loaded < 40,
        "{loaded} slices for twelve short cells"
    );
    let again = session.pdf_font_needs(0).unwrap();
    let again_wants: Vec<(Variant, char)> = again
        .iter()
        .flat_map(|(v, chars)| chars.chars().map(move |c| (*v, c)))
        .collect();
    assert_eq!(
        dir.load(again_wants),
        0,
        "nothing further to load: {again:?}"
    );

    let (pdf, report) = session.export_pdf_with_report(0).unwrap();
    assert!(
        !report
            .entries()
            .iter()
            .any(|e| e.feature.contains("no installed face")),
        "everything drawn: {:?}",
        report.entries()
    );
    if let Some(path) = std::env::var_os("OPENCALC_DUMP_CJK_PDF") {
        std::fs::write(path, &pdf).unwrap();
        eprintln!(
            "loaded {loaded} slices; embedded {} bytes",
            embedded_font_bytes(&pdf)
        );
    }
    let text = extract(&pdf);
    for row in ROWS {
        // Spaces between runs are the extractor's, not the cell's.
        let squeezed: String = text.chars().filter(|c| *c != ' ').collect();
        let want: String = row.chars().filter(|c| *c != ' ').collect();
        assert!(
            squeezed.contains(&want),
            "{row:?} does not extract from the PDF; got {text:?}"
        );
    }

    // The bold ＭＳ 明朝 row is emboldened (fill and stroke), as Windows draws a
    // bold the face does not have.
    assert!(
        String::from_utf8_lossy(&pdf).contains("q 2 Tr "),
        "no emboldened run in the PDF"
    );

    // Subset: only the glyphs drawn, not the slices whole.
    let embedded = embedded_font_bytes(&pdf);
    assert!(
        embedded < 400_000,
        "{embedded} bytes of embedded font for twelve short cells"
    );

    // And the same bytes twice.
    assert_eq!(pdf, session.export_pdf(0).unwrap());

    // If poppler is installed, it agrees.
    if let Ok(out) = std::process::Command::new("pdftotext")
        .args(["-", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(&pdf)?;
            child.wait_with_output()
        })
    {
        let got = String::from_utf8_lossy(&out.stdout).replace([' ', '\n'], "");
        for row in ROWS {
            let want: String = row.chars().filter(|c| *c != ' ').collect();
            assert!(got.contains(&want), "pdftotext lost {row:?}: {got:?}");
        }
    }
}

/// The saved file keeps the font names it was given: the mapping decides what
/// draws a cell, never what the file says.
#[test]
fn east_asian_font_names_survive_a_save() {
    let mut session = fixture();
    // An edit, so the save is written rather than handed back verbatim.
    let op = session.input_edit(0, casual_calc_model::CellRef::new(20, 0), "x");
    session.edit(op).unwrap();
    let saved = session.save().unwrap();
    let reopened = WorkbookSession::open(saved).unwrap();
    let names: Vec<String> = reopened
        .workbook()
        .styles
        .iter()
        .filter_map(|s| s.font_name.clone())
        .collect();
    for name in [
        "游ゴシック",
        "ＭＳ 明朝",
        "等线",
        "宋体",
        "微軟正黑體",
        "新細明體",
        "맑은 고딕",
        "바탕",
        "ＭＳ Ｐゴシック",
    ] {
        assert!(
            names.iter().any(|n| n == name),
            "{name} lost on save: {names:?}"
        );
    }
}
