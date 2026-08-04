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

use std::io::{Cursor, Write};

use casual_calc_formula::column_to_letters;
use casual_calc_model::{Cell, CellRange, CellValue, SheetId, Workbook};
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
    let mut s = format!("{DECL}<styleSheet xmlns=\"{NS_MAIN}\">");
    s.push_str(&format!("<numFmts count=\"{}\">", styles.len()));
    for (j, style) in styles.iter().enumerate() {
        if let Some(code) = &style.number_format {
            s.push_str(&format!(
                "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                FIRST_CUSTOM_NUM_FMT + j as u32,
                escape_attr(code)
            ));
        }
    }
    s.push_str("</numFmts>");
    // cellXfs: index 0 = General, then one xf per interned style (index j+1).
    s.push_str(&format!("<cellXfs count=\"{}\">", styles.len() + 1));
    s.push_str("<xf numFmtId=\"0\"/>");
    for j in 0..styles.len() {
        s.push_str(&format!(
            "<xf numFmtId=\"{}\" applyNumberFormat=\"1\"/>",
            FIRST_CUSTOM_NUM_FMT + j as u32
        ));
    }
    s.push_str("</cellXfs></styleSheet>");
    s
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

    s.push_str("<sheetData>");
    let mut current_row: Option<u32> = None;
    for (at, cell) in sheet.cells.iter() {
        if current_row != Some(at.row) {
            if current_row.is_some() {
                s.push_str("</row>");
            }
            s.push_str(&format!("<row r=\"{}\">", at.row + 1));
            current_row = Some(at.row);
        }
        write_cell(&mut s, workbook, at.row, at.col, cell);
    }
    if current_row.is_some() {
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
