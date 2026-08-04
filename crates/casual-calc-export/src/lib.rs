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

use std::collections::BTreeSet;
use std::io::{Cursor, Write};

use casual_calc_formula::column_to_letters;
use casual_calc_model::{BorderEdge, Borders, Cell, CellRange, CellValue, SheetId, Workbook};
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
    let has_styles = !workbook.styles.is_empty();

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
        parts.push(("xl/styles.xml".to_owned(), styles_xml(workbook)));
    }
    for i in 0..workbook.sheets.len() {
        parts.push((
            format!("xl/worksheets/sheet{}.xml", i + 1),
            worksheet_xml(workbook, i),
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
    let mut s = format!("{DECL}<Types xmlns=\"{NS_CT}\">");
    s.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    s.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
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
    s.push_str("</Types>");
    s
}

fn root_rels() -> String {
    format!(
        "{DECL}<Relationships xmlns=\"{NS_REL}\"><Relationship Id=\"rId1\" Type=\"{NS_R}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    )
}

fn workbook_xml(workbook: &Workbook) -> String {
    let mut s = format!("{DECL}<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\"><sheets>");
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        s.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
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

fn styles_xml(workbook: &Workbook) -> String {
    let styles: Vec<_> = workbook.styles.iter().collect();

    // Deduplicate fonts, solid fills, and custom number formats, and record the
    // (fontId, fillId, numFmtId) each interned style resolves to. Fill ids 0 and
    // 1 are reserved (none / gray125); font id 0 is the default font.
    let mut fonts: Vec<(bool, bool, Option<String>)> = vec![(false, false, None)];
    let mut fills: Vec<String> = Vec::new();
    let mut num_codes: Vec<String> = Vec::new();
    // Border id 0 is reserved for the empty border; interned borders start at 1.
    let mut borders: Vec<Borders> = Vec::new();
    let mut per_style: Vec<StyleIds> = Vec::with_capacity(styles.len());

    for style in &styles {
        let font_key = (style.bold, style.italic, style.font_color.clone());
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
        per_style.push(StyleIds {
            font_id,
            fill_id,
            num_fmt_id,
            border_id,
        });
    }

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
    for (bold, italic, color) in &fonts {
        s.push_str("<font>");
        if *bold {
            s.push_str("<b/>");
        }
        if *italic {
            s.push_str("<i/>");
        }
        if let Some(c) = color {
            s.push_str(&format!("<color rgb=\"FF{c}\"/>"));
        }
        s.push_str("<sz val=\"11\"/><name val=\"Calibri\"/></font>");
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
    s.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");

    s.push_str(&format!("<cellXfs count=\"{}\">", styles.len() + 1));
    s.push_str("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>");
    for ids in &per_style {
        let apply_num = if ids.num_fmt_id != 0 {
            " applyNumberFormat=\"1\""
        } else {
            ""
        };
        let apply_font = if ids.font_id != 0 {
            " applyFont=\"1\""
        } else {
            ""
        };
        let apply_fill = if ids.fill_id != 0 {
            " applyFill=\"1\""
        } else {
            ""
        };
        let apply_border = if ids.border_id != 0 {
            " applyBorder=\"1\""
        } else {
            ""
        };
        s.push_str(&format!(
            "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\" xfId=\"0\"{apply_num}{apply_font}{apply_fill}{apply_border}/>",
            ids.num_fmt_id, ids.font_id, ids.fill_id, ids.border_id
        ));
    }
    s.push_str("</cellXfs></styleSheet>");
    s
}

/// The resolved OOXML style-collection ids a single interned style maps to.
struct StyleIds {
    font_id: usize,
    fill_id: usize,
    num_fmt_id: u32,
    border_id: usize,
}

fn write_border(s: &mut String, border: &Borders) {
    s.push_str("<border>");
    write_border_edge(s, "left", &border.left);
    write_border_edge(s, "right", &border.right);
    write_border_edge(s, "top", &border.top);
    write_border_edge(s, "bottom", &border.bottom);
    s.push_str("<diagonal/>");
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

fn worksheet_xml(workbook: &Workbook, sheet_index: usize) -> String {
    let sheet = &workbook.sheets[sheet_index];
    let mut s = format!("{DECL}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\">");

    if !sheet.view.is_default() {
        let top_left = cell_a1(sheet.view.frozen_rows, sheet.view.frozen_cols);
        s.push_str(&format!(
            "<sheetViews><sheetView workbookViewId=\"0\"><pane xSplit=\"{}\" ySplit=\"{}\" topLeftCell=\"{}\" state=\"frozen\" activePane=\"bottomRight\"/></sheetView></sheetViews>",
            sheet.view.frozen_cols, sheet.view.frozen_rows, top_left
        ));
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
    if !sheet.columns.sizes.is_empty() {
        s.push_str("<cols>");
        let entries: Vec<(u32, i64)> = sheet.columns.sizes.iter().map(|(&k, &v)| (k, v)).collect();
        let mut i = 0;
        while i < entries.len() {
            let (start, width) = entries[i];
            let mut end = start;
            let mut j = i + 1;
            // Coalesce a run of consecutive equal-width columns into one span.
            while j < entries.len() && entries[j].0 == end + 1 && entries[j].1 == width {
                end = entries[j].0;
                j += 1;
            }
            s.push_str(&format!(
                "<col min=\"{}\" max=\"{}\" width=\"{}\" customWidth=\"1\"/>",
                start + 1,
                end + 1,
                fmt_f64(twips_to_col_chars(width))
            ));
            i = j;
        }
        s.push_str("</cols>");
    }

    s.push_str("<sheetData>");
    // The rows to emit: every row with cells, plus any row carrying a custom
    // height (even if it has no cells). Cells iterate in row-major order.
    let mut rows: BTreeSet<u32> = sheet.rows.sizes.keys().copied().collect();
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
        s.push_str(&format!("<row r=\"{}\"{ht_attr}>", row + 1));
        while cells.peek().is_some_and(|(at, _)| at.row == row) {
            let (at, cell) = cells.next().unwrap();
            write_cell(&mut s, workbook, at.row, at.col, cell);
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");

    if !sheet.merges.is_empty() {
        s.push_str(&format!("<mergeCells count=\"{}\">", sheet.merges.len()));
        for range in &sheet.merges {
            s.push_str(&format!("<mergeCell ref=\"{}\"/>", range_a1(range)));
        }
        s.push_str("</mergeCells>");
    }

    s.push_str("</worksheet>");
    s
}

fn write_cell(s: &mut String, workbook: &Workbook, row: u32, col: u32, cell: &Cell) {
    let reference = cell_a1(row, col);
    let style_attr = cell
        .style
        .and_then(|id| workbook.styles.index_of(id))
        .map(|i| format!(" s=\"{}\"", i + 1))
        .unwrap_or_default();

    let type_attr = match cell.value {
        CellValue::Bool(_) => " t=\"b\"",
        CellValue::SharedString(_) => " t=\"s\"",
        CellValue::InlineString(_) => " t=\"inlineStr\"",
        CellValue::Error(_) => " t=\"e\"",
        _ => "",
    };

    s.push_str(&format!("<c r=\"{reference}\"{style_attr}{type_attr}>"));

    if let Some(handle) = cell.formula
        && let Some(expr) = workbook.formula(handle)
    {
        s.push_str(&format!("<f>{}</f>", escape_text(&expr.to_string())));
    }

    match &cell.value {
        CellValue::Empty => {}
        CellValue::Number(n) => s.push_str(&format!("<v>{n}</v>")),
        CellValue::Bool(b) => s.push_str(&format!("<v>{}</v>", if *b { 1 } else { 0 })),
        CellValue::Error(e) => s.push_str(&format!("<v>{e}</v>")),
        CellValue::SharedString(id) => {
            let index = workbook.strings.index_of(*id).unwrap_or(0);
            s.push_str(&format!("<v>{index}</v>"));
        }
        CellValue::InlineString(id) => {
            let text = workbook.strings.get(*id).unwrap_or("");
            s.push_str(&format!(
                "<is><t xml:space=\"preserve\">{}</t></is>",
                escape_text(text)
            ));
        }
    }

    s.push_str("</c>");
}

fn sheet_index(workbook: &Workbook, id: SheetId) -> Option<usize> {
    workbook.sheets.iter().position(|s| s.id == id)
}

#[cfg(test)]
mod tests;
