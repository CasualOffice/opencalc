//! `casual-calc-export` — the semantic SpreadsheetML writer.
//!
//! Phase 1B: serializes a normalized [`Workbook`] back to a valid, deterministic
//! `.xlsx` package — cell values, formulas (from the AST), number formats,
//! merged ranges, frozen panes, and defined names. The output is a *semantic*
//! reconstruction (canonical OOXML), not a byte-identical copy of an original
//! (that is the retention-mode repackager, a later increment). The guarantee is
//! the **semantic fixed point**: `import → write → import` yields an equal model.
//!
//! See `docs/36-EXPORT-AND-ROUNDTRIP-DESIGN.md`.

mod error;
mod xml;

pub use error::ExportError;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use casual_calc_formula::column_to_letters;
use casual_calc_model::{
    BorderEdge, Borders, Cell, CellRange, CellValue, CfRule, ConditionalFormat, ErrorValue,
    FilterRule, HAlign, Sheet, SheetId, Style, VAlign, Workbook,
};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use xml::{escape_attr, escape_text};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const FIRST_CUSTOM_NUM_FMT: u32 = 164;
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

/// Serialize a workbook to a deterministic `.xlsx` package.
pub fn write_workbook(workbook: &Workbook) -> Result<Vec<u8>, ExportError> {
    let has_strings = !workbook.strings.is_empty();
    // Conditional-format fills become `<dxfs>` in styles.xml, shared by dxfId
    // with the worksheet `<cfRule>`s — so styles.xml is written when there are
    // dxfs even if no cell carries a style.
    let dxfs = collect_dxfs(workbook);
    let has_styles = !workbook.styles.is_empty() || !dxfs.is_empty();

    let mut parts: Vec<(String, String)> = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types(workbook, has_styles, has_strings),
        ),
        ("_rels/.rels".to_owned(), root_rels()),
        ("xl/workbook.xml".to_owned(), workbook_xml(workbook)),
        (
            "xl/_rels/workbook.xml.rels".to_owned(),
            workbook_rels(workbook, has_styles, has_strings),
        ),
    ];
    if has_strings {
        parts.push((
            "xl/sharedStrings.xml".to_owned(),
            shared_strings_xml(workbook),
        ));
    }
    if has_styles {
        parts.push(("xl/styles.xml".to_owned(), styles_xml(workbook, &dxfs)));
    }
    for i in 0..workbook.sheets.len() {
        parts.push((
            format!("xl/worksheets/sheet{}.xml", i + 1),
            worksheet_xml(workbook, i, &dxfs),
        ));
    }
    // Comment parts: a comments part, a legacy VML drawing (so Excel renders the
    // note markers), and the per-sheet rels that tie them to the worksheet.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        if sheet.comments.is_empty() {
            continue;
        }
        let n = i + 1;
        parts.push((format!("xl/comments{n}.xml"), comments_xml(sheet)));
        parts.push((format!("xl/drawings/vmlDrawing{n}.vml"), vml_drawing(sheet)));
        parts.push((
            format!("xl/worksheets/_rels/sheet{n}.xml.rels"),
            sheet_rels(n),
        ));
    }

    package(&parts)
}

fn package(parts: &[(String, String)]) -> Result<Vec<u8>, ExportError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (path, content) in parts {
        writer.start_file(path, options)?;
        writer.write_all(content.as_bytes())?;
    }
    Ok(writer.finish()?.into_inner())
}

fn content_types(workbook: &Workbook, has_styles: bool, has_strings: bool) -> String {
    let any_comments = workbook.sheets.iter().any(|s| !s.comments.is_empty());
    let mut s = format!("{DECL}<Types xmlns=\"{NS_CT}\">");
    s.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    s.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    if any_comments {
        s.push_str("<Default Extension=\"vml\" ContentType=\"application/vnd.openxmlformats-officedocument.vmlDrawing\"/>");
    }
    s.push_str("<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>");
    for i in 0..workbook.sheets.len() {
        s.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i + 1
        ));
    }
    if has_styles {
        s.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>");
    }
    if has_strings {
        s.push_str("<Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>");
    }
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        if !sheet.comments.is_empty() {
            s.push_str(&format!(
                "<Override PartName=\"/xl/comments{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml\"/>",
                i + 1
            ));
        }
    }
    s.push_str("</Types>");
    s
}

/// The `xl/comments{n}.xml` part: authors + one `<comment>` per note.
fn comments_xml(sheet: &Sheet) -> String {
    let mut authors: Vec<String> = Vec::new();
    for c in &sheet.comments {
        let a = c.author.clone().unwrap_or_else(|| "OpenCalc".to_owned());
        if !authors.contains(&a) {
            authors.push(a);
        }
    }
    if authors.is_empty() {
        authors.push("OpenCalc".to_owned());
    }
    let mut s = format!("{DECL}<comments xmlns=\"{NS_MAIN}\"><authors>");
    for a in &authors {
        s.push_str(&format!("<author>{}</author>", escape_text(a)));
    }
    s.push_str("</authors><commentList>");
    for c in &sheet.comments {
        let a = c.author.clone().unwrap_or_else(|| "OpenCalc".to_owned());
        let aid = authors.iter().position(|x| *x == a).unwrap_or(0);
        s.push_str(&format!(
            "<comment ref=\"{}\" authorId=\"{aid}\"><text><r><t xml:space=\"preserve\">{}</t></r></text></comment>",
            cell_a1(c.at.row, c.at.col),
            escape_text(&c.text)
        ));
    }
    s.push_str("</commentList></comments>");
    s
}

/// A minimal legacy VML drawing anchoring a note marker at each commented cell,
/// so Excel shows the red indicator. The comment text lives in comments{n}.xml.
fn vml_drawing(sheet: &Sheet) -> String {
    let mut s = String::from(
        "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\">\
<o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"1\"/></o:shapelayout>\
<v:shapetype id=\"_x0000_t202\" coordsize=\"21600,21600\" o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/><v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>",
    );
    for (idx, c) in sheet.comments.iter().enumerate() {
        s.push_str(&format!(
            "<v:shape id=\"_x0000_s{}\" type=\"#_x0000_t202\" style=\"position:absolute;visibility:hidden\" fillcolor=\"#ffffe1\" o:insetmode=\"auto\">\
<v:fill color2=\"#ffffe1\"/><v:shadow on=\"t\" color=\"black\" obscured=\"t\"/><v:path o:connecttype=\"none\"/>\
<x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:SizeWithCells/><x:AutoFill>False</x:AutoFill><x:Row>{}</x:Row><x:Column>{}</x:Column></x:ClientData>\
</v:shape>",
            1025 + idx,
            c.at.row,
            c.at.col
        ));
    }
    s.push_str("</xml>");
    s
}

/// The `xl/worksheets/_rels/sheet{n}.xml.rels`: links the VML drawing and the
/// comments part to the worksheet.
fn sheet_rels(n: usize) -> String {
    format!(
        "{DECL}<Relationships xmlns=\"{NS_REL}\">\
<Relationship Id=\"rId1\" Type=\"{NS_R}/vmlDrawing\" Target=\"../drawings/vmlDrawing{n}.vml\"/>\
<Relationship Id=\"rId2\" Type=\"{NS_R}/comments\" Target=\"../comments{n}.xml\"/>\
</Relationships>"
    )
}

fn root_rels() -> String {
    format!(
        "{DECL}<Relationships xmlns=\"{NS_REL}\"><Relationship Id=\"rId1\" Type=\"{NS_R}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    )
}

fn workbook_xml(workbook: &Workbook) -> String {
    let mut s = format!("{DECL}<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\"><sheets>");
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        // Visible is the schema default, so it is written by omission.
        let state = sheet
            .visibility
            .ooxml()
            .map(|v| format!(" state=\"{v}\""))
            .unwrap_or_default();
        s.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\"{state} r:id=\"rId{}\"/>",
            escape_attr(&sheet.name),
            i + 1,
            i + 1
        ));
    }
    s.push_str("</sheets>");
    if !workbook.defined_names.is_empty() {
        s.push_str("<definedNames>");
        for name in &workbook.defined_names {
            let scope = name
                .sheet
                .and_then(|id| sheet_index(workbook, id))
                .map(|i| format!(" localSheetId=\"{i}\""))
                .unwrap_or_default();
            s.push_str(&format!(
                "<definedName name=\"{}\"{scope}>{}</definedName>",
                escape_attr(&name.name),
                escape_text(&name.formula.to_string())
            ));
        }
        s.push_str("</definedNames>");
    }
    s.push_str("</workbook>");
    s
}

fn workbook_rels(workbook: &Workbook, has_styles: bool, has_strings: bool) -> String {
    let mut s = format!("{DECL}<Relationships xmlns=\"{NS_REL}\">");
    let mut next_rid = workbook.sheets.len() + 1;
    for i in 0..workbook.sheets.len() {
        s.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"{NS_R}/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
    }
    if has_styles {
        s.push_str(&format!(
            "<Relationship Id=\"rId{next_rid}\" Type=\"{NS_R}/styles\" Target=\"styles.xml\"/>"
        ));
        next_rid += 1;
    }
    if has_strings {
        s.push_str(&format!(
            "<Relationship Id=\"rId{next_rid}\" Type=\"{NS_R}/sharedStrings\" Target=\"sharedStrings.xml\"/>"
        ));
    }
    s.push_str("</Relationships>");
    s
}

fn shared_strings_xml(workbook: &Workbook) -> String {
    let count = workbook.strings.len();
    let mut s =
        format!("{DECL}<sst xmlns=\"{NS_MAIN}\" count=\"{count}\" uniqueCount=\"{count}\">");
    for text in workbook.strings.iter() {
        s.push_str(&format!(
            "<si><t xml:space=\"preserve\">{}</t></si>",
            escape_text(text)
        ));
    }
    s.push_str("</sst>");
    s
}

/// The deduplicated conditional-format fill colors across the workbook — each
/// becomes one `<dxf>` (differential format), indexed by position (the dxfId).
fn collect_dxfs(workbook: &Workbook) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for sheet in &workbook.sheets {
        for cf in &sheet.conditional_formats {
            if !out.contains(&cf.fill) {
                out.push(cf.fill.clone());
            }
        }
    }
    out
}

/// Write one `<xf>`. Shared by `cellStyleXfs` (which carries no `xfId`) and
/// `cellXfs` (which points at the named style it belongs to), so the two can
/// never disagree about how a format is spelled.
fn write_xf(s: &mut String, ids: &StyleIds, xf_id: Option<usize>) {
    let flag = |on: bool, attr: &'static str| if on { attr } else { "" };
    let has_align = ids.align.is_some()
        || ids.valign.is_some()
        || ids.wrap
        || ids.indent != 0
        || ids.rotation != 0;
    let xf_attr = xf_id
        .map(|id| format!(" xfId=\"{id}\""))
        .unwrap_or_default();
    s.push_str(&format!(
        "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\"{xf_attr}{}{}{}{}{}",
        ids.num_fmt_id,
        ids.font_id,
        ids.fill_id,
        ids.border_id,
        flag(ids.num_fmt_id != 0, " applyNumberFormat=\"1\""),
        flag(ids.font_id != 0, " applyFont=\"1\""),
        flag(ids.fill_id != 0, " applyFill=\"1\""),
        flag(ids.border_id != 0, " applyBorder=\"1\""),
        flag(has_align, " applyAlignment=\"1\""),
    ));
    if !has_align {
        s.push_str("/>");
        return;
    }
    s.push_str("><alignment");
    if let Some(align) = ids.align {
        s.push_str(&format!(" horizontal=\"{}\"", align.ooxml()));
    }
    if let Some(valign) = ids.valign {
        s.push_str(&format!(" vertical=\"{}\"", valign.ooxml()));
    }
    if ids.wrap {
        s.push_str(" wrapText=\"1\"");
    }
    if ids.indent != 0 {
        s.push_str(&format!(" indent=\"{}\"", ids.indent));
    }
    if ids.rotation != 0 {
        s.push_str(&format!(" textRotation=\"{}\"", ids.rotation));
    }
    s.push_str("/></xf>");
}

fn styles_xml(workbook: &Workbook, dxfs: &[String]) -> String {
    let styles: Vec<_> = workbook.styles.iter().collect();

    // Deduplicate fonts, solid fills, and custom number formats, and record the
    // (fontId, fillId, numFmtId) each interned style resolves to. Fill ids 0 and
    // 1 are reserved (none / gray125); font id 0 is the default font.
    // Font key: (bold, italic, underline, strike, color, name, size_hp).
    let mut fonts: Vec<FontKey> = vec![(false, false, false, false, None, None, None)];
    let mut fills: Vec<String> = Vec::new();
    let mut num_codes: Vec<String> = Vec::new();
    // Border id 0 is reserved for the empty border; interned borders start at 1.
    let mut borders: Vec<Borders> = Vec::new();
    let mut per_style: Vec<StyleIds> = Vec::with_capacity(styles.len());

    let mut intern = |style: &Style| {
        let font_key = (
            style.bold,
            style.italic,
            style.underline,
            style.strike,
            style.font_color.clone(),
            style.font_name.clone(),
            style.font_size_hp,
        );
        let font_id = fonts
            .iter()
            .position(|f| f == &font_key)
            .unwrap_or_else(|| {
                fonts.push(font_key.clone());
                fonts.len() - 1
            });
        let fill_id = match &style.fill_color {
            Some(color) => {
                2 + fills.iter().position(|f| f == color).unwrap_or_else(|| {
                    fills.push(color.clone());
                    fills.len() - 1
                })
            }
            None => 0,
        };
        let num_fmt_id = match &style.number_format {
            Some(code) => {
                let idx = num_codes.iter().position(|c| c == code).unwrap_or_else(|| {
                    num_codes.push(code.clone());
                    num_codes.len() - 1
                });
                FIRST_CUSTOM_NUM_FMT + idx as u32
            }
            None => 0,
        };
        let border_id = match &style.border {
            Some(b) if !b.is_empty() => {
                1 + borders.iter().position(|x| x == b).unwrap_or_else(|| {
                    borders.push(b.clone());
                    borders.len() - 1
                })
            }
            _ => 0,
        };
        StyleIds {
            font_id,
            fill_id,
            num_fmt_id,
            border_id,
            align: style.align,
            valign: style.valign,
            wrap: style.wrap,
            indent: style.indent,
            rotation: style.rotation,
        }
    };
    for style in &styles {
        per_style.push(intern(style));
    }
    // Named styles resolve through the same tables, so a font only a named style
    // uses still lands in `<fonts>`.
    //
    // OOXML requires `cellStyleXfs[0]` to be the Normal style — it is what every
    // unlinked cell's `xfId="0"` points at. Our own ordering is whatever the
    // source file's `<cellStyles>` happened to be, so emit Normal first and remap
    // the links rather than assuming it was already there.
    let normal = workbook
        .cell_styles
        .iter()
        .position(|cs| cs.builtin_id == Some(0) || cs.name.eq_ignore_ascii_case("Normal"));
    let mut order: Vec<usize> = normal.into_iter().collect();
    order.extend((0..workbook.cell_styles.len()).filter(|i| Some(*i) != normal));
    // `slot[i]` is where cell_styles[i] ends up in cellStyleXfs.
    let mut slot = vec![0usize; workbook.cell_styles.len()];
    for (pos, &i) in order.iter().enumerate() {
        slot[i] = pos;
    }
    let per_named: Vec<StyleIds> = order
        .iter()
        .map(|&i| intern(&workbook.cell_styles[i].style))
        .collect();

    let mut s = format!("{DECL}<styleSheet xmlns=\"{NS_MAIN}\">");
    if !num_codes.is_empty() {
        s.push_str(&format!("<numFmts count=\"{}\">", num_codes.len()));
        for (i, code) in num_codes.iter().enumerate() {
            s.push_str(&format!(
                "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                FIRST_CUSTOM_NUM_FMT + i as u32,
                escape_attr(code)
            ));
        }
        s.push_str("</numFmts>");
    }

    s.push_str(&format!("<fonts count=\"{}\">", fonts.len()));
    for (bold, italic, underline, strike, color, name, size_hp) in &fonts {
        s.push_str("<font>");
        if *bold {
            s.push_str("<b/>");
        }
        if *italic {
            s.push_str("<i/>");
        }
        if *underline {
            s.push_str("<u/>");
        }
        if *strike {
            s.push_str("<strike/>");
        }
        if let Some(c) = color {
            s.push_str(&format!("<color rgb=\"FF{c}\"/>"));
        }
        // Default font is Calibri 11pt (22 half-points) when unset.
        s.push_str(&format!(
            "<sz val=\"{}\"/>",
            fmt_half_points(size_hp.unwrap_or(22))
        ));
        s.push_str(&format!(
            "<name val=\"{}\"/>",
            escape_attr(name.as_deref().unwrap_or("Calibri"))
        ));
        s.push_str("</font>");
    }
    s.push_str("</fonts>");

    s.push_str(&format!("<fills count=\"{}\">", fills.len() + 2));
    s.push_str("<fill><patternFill patternType=\"none\"/></fill>");
    s.push_str("<fill><patternFill patternType=\"gray125\"/></fill>");
    for color in &fills {
        s.push_str(&format!(
            "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{color}\"/></patternFill></fill>"
        ));
    }
    s.push_str("</fills>");

    s.push_str(&format!("<borders count=\"{}\">", borders.len() + 1));
    s.push_str("<border><left/><right/><top/><bottom/><diagonal/></border>");
    for border in &borders {
        write_border(&mut s, border);
    }
    s.push_str("</borders>");
    // `<cellStyleXfs>`: the formats the named styles stand for. With no named
    // styles the single default entry is still required — `cellXfs` entries all
    // point at index 0.
    if per_named.is_empty() {
        s.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    } else {
        s.push_str(&format!("<cellStyleXfs count=\"{}\">", per_named.len()));
        for ids in &per_named {
            write_xf(&mut s, ids, None);
        }
        s.push_str("</cellStyleXfs>");
    }

    s.push_str(&format!("<cellXfs count=\"{}\">", styles.len() + 1));
    s.push_str("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>");
    for (i, ids) in per_style.iter().enumerate() {
        // The named style this cell format belongs to, remapped to its emitted
        // slot. Absent or out of range falls back to Normal.
        let xf_id = styles[i]
            .style_ref
            .and_then(|r| slot.get(r as usize).copied())
            .unwrap_or(0);
        write_xf(&mut s, ids, Some(xf_id));
    }
    s.push_str("</cellXfs>");

    // `<cellStyles>` names the cellStyleXfs entries. It sits between cellXfs and
    // dxfs in the CT_Stylesheet sequence.
    if !order.is_empty() {
        s.push_str(&format!("<cellStyles count=\"{}\">", order.len()));
        for (pos, &i) in order.iter().enumerate() {
            let cs = &workbook.cell_styles[i];
            let builtin = cs
                .builtin_id
                .map(|b| format!(" builtinId=\"{b}\""))
                .unwrap_or_default();
            s.push_str(&format!(
                "<cellStyle name=\"{}\" xfId=\"{pos}\"{builtin}/>",
                escape_attr(&cs.name)
            ));
        }
        s.push_str("</cellStyles>");
    }

    // Differential formats (conditional-format fills), after cellXfs.
    if !dxfs.is_empty() {
        s.push_str(&format!("<dxfs count=\"{}\">", dxfs.len()));
        for fill in dxfs {
            s.push_str(&format!(
                "<dxf><fill><patternFill><bgColor rgb=\"FF{fill}\"/></patternFill></fill></dxf>"
            ));
        }
        s.push_str("</dxfs>");
    }
    s.push_str("</styleSheet>");
    s
}

/// A deduplication key for a `<font>`: (bold, italic, underline, strike, color,
/// name, size in half-points).
type FontKey = (
    bool,
    bool,
    bool,
    bool,
    Option<String>,
    Option<String>,
    Option<u32>,
);

/// The resolved OOXML style-collection ids a single interned style maps to.
struct StyleIds {
    font_id: usize,
    fill_id: usize,
    num_fmt_id: u32,
    border_id: usize,
    align: Option<HAlign>,
    valign: Option<VAlign>,
    wrap: bool,
    indent: u8,
    rotation: u16,
}

/// The per-column attributes coalesced into one `<col>` span: a custom width
/// (twips), a hidden flag, an outline nesting level, and a collapsed flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ColAttrs {
    width: Option<i64>,
    hidden: bool,
    outline_level: u8,
    collapsed: bool,
}

fn write_border(s: &mut String, border: &Borders) {
    // The diagonal directions are attributes of `<border>`; the line's style and
    // colour live in the `<diagonal>` child.
    s.push_str("<border");
    if border.diagonal_up {
        s.push_str(" diagonalUp=\"1\"");
    }
    if border.diagonal_down {
        s.push_str(" diagonalDown=\"1\"");
    }
    s.push('>');
    write_border_edge(s, "left", &border.left);
    write_border_edge(s, "right", &border.right);
    write_border_edge(s, "top", &border.top);
    write_border_edge(s, "bottom", &border.bottom);
    write_border_edge(s, "diagonal", &border.diagonal);
    s.push_str("</border>");
}

fn write_border_edge(s: &mut String, name: &str, edge: &Option<BorderEdge>) {
    match edge {
        Some(edge) => {
            s.push_str(&format!("<{name} style=\"{}\">", escape_attr(&edge.style)));
            if let Some(color) = &edge.color {
                s.push_str(&format!("<color rgb=\"FF{color}\"/>"));
            }
            s.push_str(&format!("</{name}>"));
        }
        None => s.push_str(&format!("<{name}/>")),
    }
}

fn cell_a1(row: u32, col: u32) -> String {
    format!("{}{}", column_to_letters(col), row + 1)
}

fn range_a1(range: &CellRange) -> String {
    format!(
        "{}:{}",
        cell_a1(range.start.row, range.start.col),
        cell_a1(range.end.row, range.end.col)
    )
}

/// Reverse of the importer's column-width conversion: twips → Excel character
/// width. Chosen so `read(write(x)) == x` for import-derived widths.
fn twips_to_col_chars(twips: i64) -> f64 {
    ((twips as f64 / 15.0) - 5.0) / 7.0
}

/// Reverse of the importer's row-height conversion: twips → points.
fn twips_to_row_points(twips: i64) -> f64 {
    twips as f64 / 20.0
}

/// Format a float for an XML attribute using the shortest round-trippable form.
fn fmt_f64(value: f64) -> String {
    format!("{value}")
}

/// Render a half-point font size as OOXML points: an integral number of points
/// prints with no fraction (`22` → `11`), a half-point keeps `.5` (`23` → `11.5`).
fn fmt_half_points(size_hp: u32) -> String {
    if size_hp.is_multiple_of(2) {
        format!("{}", size_hp / 2)
    } else {
        format!("{}.5", size_hp / 2)
    }
}

fn worksheet_xml(workbook: &Workbook, sheet_index: usize, dxfs: &[String]) -> String {
    let sheet = &workbook.sheets[sheet_index];
    let mut s = format!("{DECL}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\">");

    // `<sheetPr>` is first in the CT_Worksheet sequence, and within it the schema
    // order is `tabColor` then `outlinePr`. Excel stores the tab color as 8-hex
    // ARGB; the model keeps `RRGGBB`, so we prepend an opaque `FF` alpha on the
    // way out. `<outlinePr>` is emitted only for non-default summary positions.
    let has_outline_pr = !sheet.outline.is_default();
    if sheet.tab_color.is_some() || has_outline_pr {
        s.push_str("<sheetPr>");
        if let Some(rgb) = &sheet.tab_color {
            s.push_str(&format!(
                "<tabColor rgb=\"FF{}\"/>",
                rgb.to_ascii_uppercase()
            ));
        }
        if has_outline_pr {
            s.push_str("<outlinePr");
            if !sheet.outline.summary_below {
                s.push_str(" summaryBelow=\"0\"");
            }
            if !sheet.outline.summary_right {
                s.push_str(" summaryRight=\"0\"");
            }
            s.push_str("/>");
        }
        s.push_str("</sheetPr>");
    }

    // `<sheetView>` carries the zoom scale (an attribute) and the frozen `<pane>`
    // (a child); either alone is enough to emit the element.
    if !sheet.view.is_default() {
        let zoom_attr = if sheet.view.zoom != 0 {
            format!(" zoomScale=\"{}\"", sheet.view.zoom)
        } else {
            String::new()
        };
        // Grid lines and headers show by default; only emit an attribute to
        // hide them, so a normal sheet writes neither.
        let grid_attr = if sheet.view.hide_gridlines {
            " showGridLines=\"0\""
        } else {
            ""
        };
        let headers_attr = if sheet.view.hide_headers {
            " showRowColHeaders=\"0\""
        } else {
            ""
        };
        s.push_str(&format!(
            "<sheetViews><sheetView{grid_attr}{headers_attr}{zoom_attr} workbookViewId=\"0\">"
        ));
        if sheet.view.frozen_rows != 0 || sheet.view.frozen_cols != 0 {
            let top_left = cell_a1(sheet.view.frozen_rows, sheet.view.frozen_cols);
            s.push_str(&format!(
                "<pane xSplit=\"{}\" ySplit=\"{}\" topLeftCell=\"{}\" state=\"frozen\" activePane=\"bottomRight\"/>",
                sheet.view.frozen_cols, sheet.view.frozen_rows, top_left
            ));
        }
        s.push_str("</sheetView></sheetViews>");
    }

    // Axis defaults, then per-column overrides (schema order: before sheetData).
    if sheet.columns.default.is_some() || sheet.rows.default.is_some() {
        s.push_str("<sheetFormatPr");
        if let Some(w) = sheet.columns.default {
            s.push_str(&format!(
                " defaultColWidth=\"{}\"",
                fmt_f64(twips_to_col_chars(w))
            ));
        }
        if let Some(h) = sheet.rows.default {
            s.push_str(&format!(
                " defaultRowHeight=\"{}\"",
                fmt_f64(twips_to_row_points(h))
            ));
        }
        s.push_str("/>");
    }
    if !sheet.columns.sizes.is_empty()
        || !sheet.hidden_cols.is_empty()
        || !sheet.col_outline_levels.is_empty()
        || !sheet.collapsed_cols.is_empty()
    {
        // Union the width overrides, hidden flags, outline levels, and collapsed
        // flags, keyed by zero-based column, so a column can carry any mix.
        let mut columns: BTreeMap<u32, ColAttrs> = BTreeMap::new();
        for (&col, &width) in &sheet.columns.sizes {
            columns.entry(col).or_default().width = Some(width);
        }
        for &col in &sheet.hidden_cols {
            columns.entry(col).or_default().hidden = true;
        }
        for (&col, &level) in &sheet.col_outline_levels {
            columns.entry(col).or_default().outline_level = level;
        }
        for &col in &sheet.collapsed_cols {
            columns.entry(col).or_default().collapsed = true;
        }
        s.push_str("<cols>");
        let entries: Vec<(u32, ColAttrs)> = columns.iter().map(|(&k, &v)| (k, v)).collect();
        let mut i = 0;
        while i < entries.len() {
            let (start, attrs) = entries[i];
            let mut end = start;
            let mut j = i + 1;
            // Coalesce a run of consecutive columns with identical attributes.
            while j < entries.len() && entries[j].0 == end + 1 && entries[j].1 == attrs {
                end = entries[j].0;
                j += 1;
            }
            let width_attr = attrs
                .width
                .map(|w| {
                    format!(
                        " width=\"{}\" customWidth=\"1\"",
                        fmt_f64(twips_to_col_chars(w))
                    )
                })
                .unwrap_or_default();
            let hidden_attr = if attrs.hidden { " hidden=\"1\"" } else { "" };
            let outline_attr = if attrs.outline_level != 0 {
                format!(" outlineLevel=\"{}\"", attrs.outline_level)
            } else {
                String::new()
            };
            let collapsed_attr = if attrs.collapsed {
                " collapsed=\"1\""
            } else {
                ""
            };
            s.push_str(&format!(
                "<col min=\"{}\" max=\"{}\"{width_attr}{hidden_attr}{outline_attr}{collapsed_attr}/>",
                start + 1,
                end + 1,
            ));
            i = j;
        }
        s.push_str("</cols>");
    }

    s.push_str("<sheetData>");
    // The rows to emit: every row with cells, plus any row carrying a custom
    // height, hidden flag, outline level, or collapsed flag (even if it has no
    // cells). Cells iterate in row-major order.
    let mut rows: BTreeSet<u32> = sheet.rows.sizes.keys().copied().collect();
    rows.extend(sheet.hidden_rows.iter().copied());
    rows.extend(sheet.filter_hidden.iter().copied());
    rows.extend(sheet.row_outline_levels.keys().copied());
    rows.extend(sheet.collapsed_rows.iter().copied());
    for (at, _) in sheet.cells.iter() {
        rows.insert(at.row);
    }
    let mut cells = sheet.cells.iter().peekable();
    for row in rows {
        let ht_attr = sheet
            .rows
            .sizes
            .get(&row)
            .map(|&t| {
                format!(
                    " ht=\"{}\" customHeight=\"1\"",
                    fmt_f64(twips_to_row_points(t))
                )
            })
            .unwrap_or_default();
        // Filtered-out rows carry hidden="1" exactly like hand-hidden ones —
        // OOXML has no separate marker, which is why the two sets are kept
        // apart in the model rather than here.
        let hidden_attr = if sheet.is_row_hidden(row) {
            " hidden=\"1\""
        } else {
            ""
        };
        let outline_attr = sheet
            .row_outline_levels
            .get(&row)
            .filter(|&&l| l != 0)
            .map(|&l| format!(" outlineLevel=\"{l}\""))
            .unwrap_or_default();
        let collapsed_attr = if sheet.collapsed_rows.contains(&row) {
            " collapsed=\"1\""
        } else {
            ""
        };
        s.push_str(&format!(
            "<row r=\"{}\"{ht_attr}{hidden_attr}{outline_attr}{collapsed_attr}>",
            row + 1
        ));
        while cells.peek().is_some_and(|(at, _)| at.row == row) {
            let (at, cell) = cells.next().unwrap();
            write_cell(&mut s, workbook, at.row, at.col, cell);
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");

    // <autoFilter> precedes <mergeCells> in the CT_Worksheet sequence.
    if let Some(filter) = &sheet.auto_filter {
        s.push_str(&format!("<autoFilter ref=\"{}\">", range_a1(&filter.range)));
        for (&col_id, rule) in &filter.rules {
            s.push_str(&format!("<filterColumn colId=\"{col_id}\">"));
            match rule {
                FilterRule::Values(vals) => {
                    // A blank is not a <filter val="">; OOXML carries it as an
                    // attribute on the container.
                    let blank = vals.iter().any(|v| v.is_empty());
                    s.push_str(if blank {
                        "<filters blank=\"1\">"
                    } else {
                        "<filters>"
                    });
                    for v in vals.iter().filter(|v| !v.is_empty()) {
                        s.push_str(&format!("<filter val=\"{}\"/>", escape_attr(v)));
                    }
                    s.push_str("</filters>");
                }
                FilterRule::Custom { first, second, and } => {
                    s.push_str(&format!(
                        "<customFilters{}>",
                        if *and { " and=\"1\"" } else { "" }
                    ));
                    for f in std::iter::once(first).chain(second.as_ref()) {
                        s.push_str(&format!(
                            "<customFilter operator=\"{}\" val=\"{}\"/>",
                            f.op.as_ooxml(),
                            escape_attr(&f.value)
                        ));
                    }
                    s.push_str("</customFilters>");
                }
            }
            s.push_str("</filterColumn>");
        }
        s.push_str("</autoFilter>");
    }

    if !sheet.merges.is_empty() {
        s.push_str(&format!("<mergeCells count=\"{}\">", sheet.merges.len()));
        for range in &sheet.merges {
            s.push_str(&format!("<mergeCell ref=\"{}\"/>", range_a1(range)));
        }
        s.push_str("</mergeCells>");
    }

    // Conditional formatting — one <conditionalFormatting> per rule, its <dxf>
    // referenced by the fill's index in the workbook dxfs list.
    for (i, cf) in sheet.conditional_formats.iter().enumerate() {
        let dxf_id = dxfs.iter().position(|f| *f == cf.fill).unwrap_or(0);
        s.push_str(&format!(
            "<conditionalFormatting sqref=\"{}\">",
            range_a1(&cf.range)
        ));
        // An explicit priority wins; zero means the rule never carried one, so
        // fall back to document order rather than writing an invalid 0.
        let priority = if cf.priority > 0 {
            cf.priority as usize
        } else {
            i + 1
        };
        s.push_str(&cf_rule_xml(cf, dxf_id, priority));
        s.push_str("</conditionalFormatting>");
    }

    // Data validations come after conditionalFormatting in the CT_Worksheet
    // sequence. A list's allowed values are an inline quoted CSV in <formula1>.
    if !sheet.validations.is_empty() {
        s.push_str(&format!(
            "<dataValidations count=\"{}\">",
            sheet.validations.len()
        ));
        for v in &sheet.validations {
            let list = v.values.join(",");
            s.push_str(&format!(
                "<dataValidation type=\"list\" allowBlank=\"1\" showInputMessage=\"1\" showErrorMessage=\"1\" sqref=\"{}\"><formula1>{}</formula1></dataValidation>",
                range_a1(&v.range),
                escape_text(&format!("\"{list}\""))
            ));
        }
        s.push_str("</dataValidations>");
    }

    // Legacy drawing ref (the VML holding note markers) — last in the sequence.
    if !sheet.comments.is_empty() {
        s.push_str("<legacyDrawing r:id=\"rId1\"/>");
    }

    s.push_str("</worksheet>");
    s
}

/// A single `<cfRule>` for a highlight-cells rule.
fn cf_rule_xml(cf: &ConditionalFormat, dxf_id: usize, priority: usize) -> String {
    let xml = cf_rule_body(cf, dxf_id, priority);
    if !cf.stop_if_true {
        return xml;
    }
    // `stopIfTrue` is an attribute of every rule form, so it is spliced in here
    // rather than repeated across a dozen format! calls.
    xml.replacen("<cfRule ", "<cfRule stopIfTrue=\"1\" ", 1)
}

fn cf_rule_body(cf: &ConditionalFormat, dxf_id: usize, priority: usize) -> String {
    match &cf.rule {
        CfRule::GreaterThan(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"greaterThan\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::LessThan(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"lessThan\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::EqualTo(x) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"equal\"><formula>{}</formula></cfRule>",
            fmt_f64(*x)
        ),
        CfRule::Between(lo, hi) => format!(
            "<cfRule type=\"cellIs\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"between\"><formula>{}</formula><formula>{}</formula></cfRule>",
            fmt_f64(*lo),
            fmt_f64(*hi)
        ),
        // Colour scales and data bars carry their own presentation, so they take
        // no dxfId — the `<cfvo>` stops describe the range's own min and max.
        CfRule::ColorScale(colors) => {
            let stops: Vec<&str> = colors.iter().map(String::as_str).collect();
            let cfvo = if stops.len() >= 3 {
                "<cfvo type=\"min\"/><cfvo type=\"percentile\" val=\"50\"/><cfvo type=\"max\"/>"
            } else {
                "<cfvo type=\"min\"/><cfvo type=\"max\"/>"
            };
            let mut colours = String::new();
            for c in &stops {
                colours.push_str(&format!("<color rgb=\"FF{}\"/>", escape_attr(c)));
            }
            format!(
                "<cfRule type=\"colorScale\" priority=\"{priority}\"><colorScale>{cfvo}{colours}</colorScale></cfRule>"
            )
        }
        CfRule::DataBar(color) => format!(
            "<cfRule type=\"dataBar\" priority=\"{priority}\"><dataBar><cfvo type=\"min\"/><cfvo type=\"max\"/><color rgb=\"FF{}\"/></dataBar></cfRule>",
            escape_attr(color)
        ),
        CfRule::Top10 {
            rank,
            bottom,
            percent,
        } => format!(
            "<cfRule type=\"top10\" dxfId=\"{dxf_id}\" priority=\"{priority}\"{}{} rank=\"{rank}\"/>",
            if *bottom { " bottom=\"1\"" } else { "" },
            if *percent { " percent=\"1\"" } else { "" },
        ),
        // The schema defaults `aboveAverage` to true, so only the "below" case
        // needs writing out.
        CfRule::AboveAverage { below, equal } => format!(
            "<cfRule type=\"aboveAverage\" dxfId=\"{dxf_id}\" priority=\"{priority}\"{}{}/>",
            if *below { " aboveAverage=\"0\"" } else { "" },
            if *equal { " equalAverage=\"1\"" } else { "" },
        ),
        CfRule::DuplicateValues { unique } => format!(
            "<cfRule type=\"{}\" dxfId=\"{dxf_id}\" priority=\"{priority}\"/>",
            if *unique {
                "uniqueValues"
            } else {
                "duplicateValues"
            },
        ),
        CfRule::TextContains(text) => {
            let top = cell_a1(cf.range.start.row, cf.range.start.col);
            format!(
                "<cfRule type=\"containsText\" dxfId=\"{dxf_id}\" priority=\"{priority}\" operator=\"containsText\" text=\"{}\"><formula>NOT(ISERROR(SEARCH(\"{}\",{top})))</formula></cfRule>",
                escape_attr(text),
                escape_text(text)
            )
        }
    }
}

fn write_cell(s: &mut String, workbook: &Workbook, row: u32, col: u32, cell: &Cell) {
    let reference = cell_a1(row, col);
    let style_attr = cell
        .style
        .and_then(|id| workbook.styles.index_of(id))
        .map(|i| format!(" s=\"{}\"", i + 1))
        .unwrap_or_default();

    let has_formula = cell.formula.is_some();
    // A non-finite number (inf/NaN — e.g. arithmetic overflow, or a `1e999`
    // literal) is not a valid OOXML numeric literal and would emit `<v>inf</v>`,
    // corrupting the file. Treat it as a #NUM! error cell for both the type
    // attribute and the value below.
    let num_error = CellValue::Error(ErrorValue::Num);
    let effective_value = match &cell.value {
        CellValue::Number(n) if !n.is_finite() => &num_error,
        other => other,
    };
    // A formula cell whose cached result is a string is a `str` type with the
    // text in `<v>` — OOXML does not allow `<is>`/shared-string on a formula
    // cell (Excel would drop the formula or repair the file). Only a *literal*
    // inline string (no formula) uses `t="inlineStr"` with `<is>`.
    let type_attr = match effective_value {
        CellValue::Bool(_) => " t=\"b\"",
        CellValue::Error(_) => " t=\"e\"",
        CellValue::SharedString(_) if has_formula => " t=\"str\"",
        CellValue::InlineString(_) if has_formula => " t=\"str\"",
        CellValue::SharedString(_) => " t=\"s\"",
        CellValue::InlineString(_) => " t=\"inlineStr\"",
        _ => "",
    };

    s.push_str(&format!("<c r=\"{reference}\"{style_attr}{type_attr}>"));

    if let Some(handle) = cell.formula
        && let Some(expr) = workbook.formula(handle)
    {
        s.push_str(&format!("<f>{}</f>", escape_text(&expr.to_string())));
    }

    match effective_value {
        CellValue::Empty => {}
        CellValue::Number(n) => s.push_str(&format!("<v>{n}</v>")),
        CellValue::Bool(b) => s.push_str(&format!("<v>{}</v>", if *b { 1 } else { 0 })),
        CellValue::Error(e) => s.push_str(&format!("<v>{e}</v>")),
        // Formula string result: emit the text in <v> (t="str"); otherwise a
        // literal shared string is emitted as its shared-table index.
        CellValue::SharedString(id) if has_formula => {
            let text = workbook.strings.get(*id).unwrap_or("");
            s.push_str(&format!("<v>{}</v>", escape_text(text)));
        }
        CellValue::SharedString(id) => {
            let index = workbook.strings.index_of(*id).unwrap_or(0);
            s.push_str(&format!("<v>{index}</v>"));
        }
        CellValue::InlineString(id) => {
            let text = workbook.strings.get(*id).unwrap_or("");
            if has_formula {
                s.push_str(&format!("<v>{}</v>", escape_text(text)));
            } else {
                s.push_str(&format!(
                    "<is><t xml:space=\"preserve\">{}</t></is>",
                    escape_text(text)
                ));
            }
        }
    }

    s.push_str("</c>");
}

fn sheet_index(workbook: &Workbook, id: SheetId) -> Option<usize> {
    workbook.sheets.iter().position(|s| s.id == id)
}

#[cfg(test)]
mod tests;
