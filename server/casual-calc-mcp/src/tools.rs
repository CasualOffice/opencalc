//! The tools (docs/97, "Tools (first set)"), each a thin wrapper over an SDK
//! call so an agent and a person get the same answers.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use casual_calc_model::{DvKind, Id, SheetVisibility};
use casual_calc_sdk::{
    Cell, CellRef, CellValue, CompatibilityReport, EditOperation, Environment, Evaluated,
    ModelOutcome, OoxmlLimits, RetentionOutcome, SessionConfig, SessionFormat, Sheet, SheetId,
    WorkbookSession,
};
use serde_json::{Map, Value, json};

use crate::a1::{self, MAX_COL, MAX_ROW, Rect, Span, cell_name, col_name, parse_col, quote_sheet};
use crate::sandbox::Sandbox;

/// At most this many workbooks open at once (docs/97, "handles are capped").
pub const MAX_HANDLES: usize = 16;

/// At most this many cells read, written or formatted per call (docs/97:
/// "`read_range` returns at most 10,000 cells per call"). The same bound on
/// writes keeps one call's undo step and reply bounded too.
pub const MAX_CELLS: u64 = 10_000;

/// The structural edits `run` accepts — exactly the ones the SDK expresses.
pub const RUN_OPS: &[&str] = &[
    "insert_rows",
    "delete_rows",
    "insert_columns",
    "delete_columns",
    "set_number_format",
    "add_sheet",
    "rename_sheet",
];

/// What a `tools/call` came to.
#[derive(Debug)]
pub enum Call {
    /// No tool by that name.
    Unknown,
    /// The tool ran: its JSON result, or the message for an `isError` result.
    Done(Result<Value, String>),
}

#[derive(Debug)]
struct Book {
    session: WorkbookSession,
    /// Canonical path of the file it was opened from or last saved to.
    path: PathBuf,
    /// The format `save` writes when not told otherwise.
    format: SessionFormat,
}

/// The open workbooks and the bounds they are opened under.
#[derive(Debug)]
pub struct Tools {
    sandbox: Sandbox,
    max_file_bytes: u64,
    books: BTreeMap<String, Book>,
    next_handle: u64,
}

type ToolResult = Result<Value, String>;

impl Tools {
    /// Tools reading and writing under `sandbox`, with the SDK's own file-size
    /// admission limit.
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            sandbox,
            max_file_bytes: OoxmlLimits::default().package.max_input_bytes,
            books: BTreeMap::new(),
            next_handle: 1,
        }
    }

    /// Lower the file-size limit. It can only go down: the SDK's limit is a
    /// security bound and nothing here may raise it.
    #[must_use]
    pub fn with_max_file_bytes(mut self, bytes: u64) -> Self {
        self.max_file_bytes = self.max_file_bytes.min(bytes);
        self
    }

    /// The file-size limit in force.
    pub fn max_file_bytes(&self) -> u64 {
        self.max_file_bytes
    }

    /// Run the tool `name`.
    pub fn call(&mut self, name: &str, args: &Value) -> Call {
        let result = match name {
            "open_workbook" => self.open_workbook(args),
            "list_sheets" => self.list_sheets(args),
            "read_range" => self.read_range(args),
            "write_cells" => self.write_cells(args),
            "evaluate" => self.evaluate(args),
            "recalculate" => self.recalculate(args),
            "run" => self.run(args),
            "save" => self.save(args),
            "close" => self.close(args),
            _ => return Call::Unknown,
        };
        Call::Done(result)
    }

    /// The `tools/list` entries.
    pub fn definitions() -> Value {
        let handle = json!({
            "type": "string",
            "description": "The handle open_workbook returned, e.g. \"wb1\"."
        });
        let sheet = json!({
            "type": "string",
            "description": "Sheet name (case-insensitive). Defaults to the first sheet, or to the sheet named in a qualified reference."
        });
        json!([
            {
                "name": "open_workbook",
                "title": "Open a workbook",
                "description": "Open a spreadsheet file (.xlsx, .xlsm, .ods, .csv, .tsv, .psv) inside the server's allowed roots and return a handle for the other tools. Relative paths are taken relative to the first allowed root. Returns the sheets with their used ranges and the import compatibility report: anything in the file this engine could not fully keep, by feature. At most 16 workbooks can be open at once.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, inside an allowed root." },
                        "format": { "type": "string", "enum": ["xlsx", "xlsm", "ods", "csv", "tsv", "psv"], "description": "Override the format the file extension implies." }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "list_sheets",
                "title": "List sheets",
                "description": "List the workbook's sheets: index, name, used range (A1), populated cell count, visibility (visible, hidden, veryHidden) and whether the sheet is protected.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "handle": handle },
                    "required": ["handle"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "read_range",
                "title": "Read a range",
                "description": "Read an A1 range (B7, A1:C10, A:C, 2:5, Sheet2!A1:B3, 'My Sheet'!A1) as a grid of rows. mode=values (default) gives each cell's value: number, string, boolean, null for empty, or {\"error\":\"#DIV/0!\"}. mode=formulas gives what the formula bar shows (\"=SUM(A1:A3)\", or the constant as typed). mode=both gives {value, formula?, display} per non-empty cell, where display is the text the grid shows with its number format applied. Whole columns/rows are clipped to the used range. At most 10,000 cells per call: a larger range is truncated to whole rows, with truncated=true and `next` listing the range(s) still to read.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "handle": handle,
                        "range": { "type": "string", "description": "A1 range, optionally sheet-qualified." },
                        "sheet": sheet,
                        "mode": { "type": "string", "enum": ["values", "formulas", "both"], "default": "values" }
                    },
                    "required": ["handle", "range"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "write_cells",
                "title": "Write cells",
                "description": "Write a 2-D array (rows of cells) with its top-left at `anchor`. A string is read exactly as typing it into Excel (en-US) would read it: \"=SUM(A1:A3)\" is a formula, \"50%\" a percentage, \"$5.00\" currency, \"12/31/2026\" a date, \"TRUE\" a boolean, a leading apostrophe forces text. Numbers and booleans are stored as such; null clears the cell. The whole call is one undo step. Cells are refused, with the reason, when the sheet is protected and the cell locked, when a formula does not parse, or when the cell's data validation (stop style) rejects the value; the rest are still written. Returns the written block's values after recalculation. At most 10,000 cells per call.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "handle": handle,
                        "anchor": { "type": "string", "description": "Top-left cell, e.g. \"B2\" or \"Sheet2!B2\"." },
                        "sheet": sheet,
                        "values": {
                            "type": "array",
                            "description": "Rows, each an array of cells (string, number, boolean or null).",
                            "items": { "type": "array", "items": { "type": ["string", "number", "boolean", "null"] } }
                        }
                    },
                    "required": ["handle", "anchor", "values"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
            },
            {
                "name": "evaluate",
                "title": "Evaluate a formula",
                "description": "Evaluate a formula against the workbook without writing it anywhere, e.g. \"=SUM(A1:A10)/COUNT(A1:A10)\" or \"=VLOOKUP(\\\"x\\\",Data!A:B,2,FALSE)\". References are read as written; unqualified ones refer to `sheet`. Returns {value}, where value is a number, string, boolean, null, {\"error\":\"#N/A\"}, or {\"array\":[[...]]} for a block result. Formula cells it depends on are computed fresh.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "handle": handle,
                        "formula": { "type": "string", "description": "The formula, with or without the leading =." },
                        "sheet": sheet
                    },
                    "required": ["handle", "formula"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": true, "openWorldHint": false }
            },
            {
                "name": "recalculate",
                "title": "Recalculate",
                "description": "Fully recalculate every formula in the workbook. Edits already recalculate what they affect unless the workbook is in manual calculation mode; use this for manual-mode workbooks or to refresh volatile functions (NOW, RAND).",
                "inputSchema": {
                    "type": "object",
                    "properties": { "handle": handle },
                    "required": ["handle"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
            },
            {
                "name": "run",
                "title": "Structural edit",
                "description": "One structural edit, one undo step. op: insert_rows / delete_rows (at = 1-based row number, count), insert_columns / delete_columns (at = column letter like \"C\" or 1-based number, count), set_number_format (range, format — an Excel number format code such as \"0.00%\", \"#,##0\", \"yyyy-mm-dd\", or \"General\"), add_sheet (name optional, position optional 1-based), rename_sheet (sheet = current name, name = new name). Formula references shift the way Excel shifts them. Sorting is not available yet. Refused on a protected sheet unless its protection allows the action, and sheet add/rename refused when the workbook structure is protected.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "handle": handle,
                        "op": { "type": "string", "enum": RUN_OPS },
                        "sheet": sheet,
                        "at": { "type": ["integer", "string"], "description": "insert/delete: 1-based row number, or column letter / 1-based column number." },
                        "count": { "type": "integer", "minimum": 1, "default": 1 },
                        "range": { "type": "string", "description": "set_number_format: A1 range." },
                        "format": { "type": "string", "description": "set_number_format: number format code." },
                        "name": { "type": "string", "description": "add_sheet / rename_sheet: the new sheet name." },
                        "position": { "type": "integer", "minimum": 1, "description": "add_sheet: 1-based tab position; default last." }
                    },
                    "required": ["handle", "op"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false }
            },
            {
                "name": "save",
                "title": "Save",
                "description": "Write the workbook to disk: to `path` (inside an allowed root) or, if omitted, back to the file it came from. The format is `format` if given, else the path's extension (.xlsx, .xlsm, .ods, .csv, .tsv, .psv); a format that contradicts the extension is refused. Returns the save-loss report: everything the chosen format cannot keep (e.g. CSV keeps one sheet's values only) — read it before telling the user the save was lossless. The write is atomic (a temporary file renamed over the target).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "handle": handle,
                        "path": { "type": "string", "description": "Target path; default the original file." },
                        "format": { "type": "string", "enum": ["xlsx", "xlsm", "ods", "csv", "tsv", "psv"] }
                    },
                    "required": ["handle"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false }
            },
            {
                "name": "close",
                "title": "Close",
                "description": "Release a workbook handle. Unsaved changes are discarded.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "handle": handle },
                    "required": ["handle"],
                    "additionalProperties": false
                },
                "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false }
            }
        ])
    }

    // -----------------------------------------------------------------------
    // open / list / close
    // -----------------------------------------------------------------------

    fn open_workbook(&mut self, args: &Value) -> ToolResult {
        let path = str_arg(args, "path")?;
        let path_in = self
            .sandbox
            .resolve_existing(path)
            .map_err(|e| e.to_string())?;
        if self.books.len() >= MAX_HANDLES {
            return Err(format!(
                "{MAX_HANDLES} workbooks are already open; close one before opening another"
            ));
        }
        let len = std::fs::metadata(&path_in)
            .map_err(|e| format!("cannot read {path}: {e}"))?
            .len();
        if len > self.max_file_bytes {
            return Err(format!(
                "{path} is {len} bytes, over the {} byte limit",
                self.max_file_bytes
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path_in)
            .and_then(|f| f.take(self.max_file_bytes + 1).read_to_end(&mut bytes))
            .map_err(|e| format!("cannot read {path}: {e}"))?;
        if bytes.len() as u64 > self.max_file_bytes {
            return Err(format!(
                "{path} grew past the {} byte limit while being read",
                self.max_file_bytes
            ));
        }
        let format = match opt_str(args, "format")? {
            Some(f) => parse_format(f)?,
            None => match format_of_path(&path_in) {
                Some(f) => f,
                None => SessionFormat::for_bytes(&bytes)
                    .ok_or_else(|| format!("cannot tell what format {path} is; pass `format`"))?,
            },
        };
        let config = SessionConfig::new().with_environment(environment_now());
        let session = WorkbookSession::open_as_with(bytes, format, config)
            .map_err(|e| format!("cannot open {path}: {e}"))?;
        let handle = format!("wb{}", self.next_handle);
        self.next_handle += 1;
        let result = json!({
            "handle": handle,
            "path": path_in.display().to_string(),
            "format": format.extension(),
            "sheets": sheets_json(&session),
            "compatibility_report": report_json(session.compatibility_report()),
        });
        self.books.insert(
            handle,
            Book {
                session,
                path: path_in,
                format,
            },
        );
        Ok(result)
    }

    fn book(&mut self, args: &Value) -> Result<&mut Book, String> {
        let handle = str_arg(args, "handle")?;
        let open: Vec<&str> = self.books.keys().map(String::as_str).collect();
        let hint = if open.is_empty() {
            "no workbook is open; call open_workbook first".to_owned()
        } else {
            format!("open handles: {}", open.join(", "))
        };
        self.books
            .get_mut(handle)
            .ok_or_else(|| format!("no open workbook {handle:?} ({hint})"))
    }

    fn list_sheets(&mut self, args: &Value) -> ToolResult {
        let book = self.book(args)?;
        Ok(json!({ "sheets": sheets_json(&book.session) }))
    }

    fn close(&mut self, args: &Value) -> ToolResult {
        let handle = str_arg(args, "handle")?.to_owned();
        self.book(args)?;
        self.books.remove(&handle);
        Ok(json!({ "closed": handle }))
    }

    // -----------------------------------------------------------------------
    // read / write / evaluate / recalculate
    // -----------------------------------------------------------------------

    fn read_range(&mut self, args: &Value) -> ToolResult {
        let mode = opt_str(args, "mode")?.unwrap_or("values");
        if !matches!(mode, "values" | "formulas" | "both") {
            return Err(format!(
                "mode must be values, formulas or both, not {mode:?}"
            ));
        }
        let range = str_arg(args, "range")?;
        let sheet_arg = opt_str(args, "sheet")?;
        let session = &self.book(args)?.session;
        let (sheet, rect) = resolve_range(session, range, sheet_arg)?;
        let name = session.workbook().sheets[sheet].name.clone();
        let Some(rect) = rect else {
            return Ok(json!({ "sheet": name, "range": null, "rows": [], "truncated": false }));
        };
        let (returned, next) = clip_to_budget(rect);
        let mut rows = Vec::new();
        for row in returned.start.row..=returned.end.row {
            let mut out = Vec::new();
            for col in returned.start.col..=returned.end.col {
                let at = CellRef::new(row, col);
                out.push(match mode {
                    "values" => value_json(session, sheet, at),
                    "formulas" => {
                        let input = session.cell_input(sheet, at);
                        if input.is_empty() {
                            Value::Null
                        } else {
                            Value::String(input)
                        }
                    }
                    _ => both_json(session, sheet, at),
                });
            }
            rows.push(Value::Array(out));
        }
        let mut result = json!({
            "sheet": name,
            "range": rect.to_string(),
            "returned": returned.to_string(),
            "truncated": !next.is_empty(),
            "rows": rows,
        });
        if !next.is_empty() {
            let quoted = quote_sheet(&name);
            result["next"] = next
                .iter()
                .map(|r| Value::String(format!("{quoted}!{r}")))
                .collect();
            result["note"] = json!(format!(
                "range has {} cells; at most {MAX_CELLS} are returned per call — read the ranges in `next` for the rest",
                rect.cells()
            ));
        }
        Ok(result)
    }

    fn write_cells(&mut self, args: &Value) -> ToolResult {
        let anchor_text = str_arg(args, "anchor")?;
        let sheet_arg = opt_str(args, "sheet")?;
        let rows = args
            .get("values")
            .and_then(Value::as_array)
            .ok_or("values must be an array of rows")?;
        let mut total: u64 = 0;
        for row in rows {
            let row = row
                .as_array()
                .ok_or("values must be an array of rows, each row an array of cells")?;
            total += row.len() as u64;
        }
        if total == 0 {
            return Err("values is empty".to_owned());
        }
        if total > MAX_CELLS {
            return Err(format!(
                "{total} cells in one call; at most {MAX_CELLS} — split the write"
            ));
        }
        let book = self.book(args)?;
        let session = &mut book.session;
        let (sheet, anchor_rect) = resolve_range(session, anchor_text, sheet_arg)?;
        let anchor = anchor_rect
            .ok_or_else(|| format!("{anchor_text:?} is not a cell"))?
            .start;

        let mut ops = Vec::new();
        let mut refused = Vec::new();
        let mut warnings = Vec::new();
        let mut end = anchor;
        for (r, row) in rows.iter().enumerate() {
            let row = row.as_array().map(Vec::as_slice).unwrap_or_default();
            for (c, input) in row.iter().enumerate() {
                let (Some(rr), Some(cc)) = (
                    u32::try_from(r)
                        .ok()
                        .and_then(|r| anchor.row.checked_add(r)),
                    u32::try_from(c)
                        .ok()
                        .and_then(|c| anchor.col.checked_add(c)),
                ) else {
                    continue;
                };
                if rr > MAX_ROW || cc > MAX_COL {
                    refused.push(json!({
                        "input": input,
                        "reason": "beyond the last row or column of the sheet",
                    }));
                    continue;
                }
                let at = CellRef::new(rr, cc);
                end = CellRef::new(end.row.max(rr), end.col.max(cc));
                match typed_op(session, sheet, at, input) {
                    Ok((op, warning)) => {
                        if let Some(w) = warning {
                            warnings.push(json!({ "cell": cell_name(at), "warning": w }));
                        }
                        ops.push(op);
                    }
                    Err(reason) => refused.push(json!({
                        "cell": cell_name(at),
                        "input": input,
                        "reason": reason,
                    })),
                }
            }
        }
        let written = ops.len();
        if !ops.is_empty() {
            session
                .edit(EditOperation::Batch(ops))
                .map_err(|e| format!("nothing was written: {e}"))?;
        }
        let block = Rect { start: anchor, end };
        let values: Vec<Value> = (block.start.row..=block.end.row)
            .map(|row| {
                Value::Array(
                    (block.start.col..=block.end.col)
                        .map(|col| value_json(session, sheet, CellRef::new(row, col)))
                        .collect(),
                )
            })
            .collect();
        let mut result = json!({
            "sheet": session.workbook().sheets[sheet].name,
            "range": block.to_string(),
            "written": written,
            "refused": refused,
            "values": values,
        });
        if !warnings.is_empty() {
            result["warnings"] = Value::Array(warnings);
        }
        if session.needs_recalculation() {
            result["needs_recalculation"] = json!(true);
        }
        Ok(result)
    }

    fn evaluate(&mut self, args: &Value) -> ToolResult {
        let formula = str_arg(args, "formula")?;
        let sheet_arg = opt_str(args, "sheet")?;
        let session = &self.book(args)?.session;
        let sheet = sheet_index(session, sheet_arg)?;
        let value = session
            .evaluate(sheet, formula)
            .map_err(|e| e.to_string())?;
        let mut result = json!({ "value": evaluated_json(&value) });
        if let Evaluated::Array { rows, cols, .. } = value
            && (rows as u64) * (cols as u64) > MAX_CELLS
        {
            result["truncated"] = json!(true);
            result["note"] = json!(format!(
                "the result is {rows}×{cols}; only the first {MAX_CELLS} cells are returned"
            ));
        }
        Ok(result)
    }

    fn recalculate(&mut self, args: &Value) -> ToolResult {
        let session = &mut self.book(args)?.session;
        session.recalculate();
        Ok(json!({ "recalculated": true }))
    }

    // -----------------------------------------------------------------------
    // run
    // -----------------------------------------------------------------------

    fn run(&mut self, args: &Value) -> ToolResult {
        let op = str_arg(args, "op")?;
        let sheet_arg = opt_str(args, "sheet")?;
        if op == "sort" {
            return Err(
                "sort is not available yet: the SDK has no sort operation for a range".to_owned(),
            );
        }
        if !RUN_OPS.contains(&op) {
            return Err(format!("unknown op {op:?}; one of: {}", RUN_OPS.join(", ")));
        }
        let book = self.book(args)?;
        let session = &mut book.session;
        match op {
            "insert_rows" | "delete_rows" | "insert_columns" | "delete_columns" => {
                let sheet = sheet_index(session, sheet_arg)?;
                let rows = op.ends_with("rows");
                let at = line_arg(args, rows)?;
                let count = count_arg(args)?;
                let max = if rows { MAX_ROW } else { MAX_COL };
                if u64::from(at) + u64::from(count) - 1 > u64::from(max) {
                    return Err("the band runs past the end of the sheet".to_owned());
                }
                let action = match op {
                    "insert_rows" => "insertRows",
                    "delete_rows" => "deleteRows",
                    "insert_columns" => "insertColumns",
                    _ => "deleteColumns",
                };
                refuse_if_protected(session, sheet, action)?;
                // The transaction layer shifts cells past the last row or
                // column rather than refusing, leaving them at addresses no
                // file can hold. Excel refuses, and so does this.
                if op.starts_with("insert")
                    && let Some(used) = used_range(&session.workbook().sheets[sheet])
                {
                    let last = if rows { used.end.row } else { used.end.col };
                    if last >= at && u64::from(last) + u64::from(count) > u64::from(max) {
                        return Err(
                            "cannot insert: it would push non-empty cells off the end of the sheet"
                                .to_owned(),
                        );
                    }
                }
                let edit = match op {
                    "insert_rows" => EditOperation::InsertRows { sheet, at, count },
                    "delete_rows" => EditOperation::DeleteRows { sheet, at, count },
                    "insert_columns" => EditOperation::InsertColumns { sheet, at, count },
                    _ => EditOperation::DeleteColumns { sheet, at, count },
                };
                session.edit(edit).map_err(|e| e.to_string())?;
                let first = if rows {
                    (at + 1).to_string()
                } else {
                    col_name(at)
                };
                let last = if rows {
                    (at + count).to_string()
                } else {
                    col_name(at + count - 1)
                };
                Ok(json!({
                    "done": op,
                    "sheet": session.workbook().sheets[sheet].name,
                    "lines": format!("{first}:{last}"),
                    "sheets": sheets_json(session),
                }))
            }
            "set_number_format" => {
                let range = str_arg(args, "range")?;
                let code = str_arg(args, "format")?;
                let (sheet, rect) = resolve_range(session, range, sheet_arg)?;
                let rect = rect.ok_or("the range is empty")?;
                if rect.cells() > MAX_CELLS {
                    return Err(format!(
                        "{} cells; at most {MAX_CELLS} per call",
                        rect.cells()
                    ));
                }
                refuse_if_protected(session, sheet, "formatCells")?;
                let general = code.trim().is_empty() || code.trim().eq_ignore_ascii_case("general");
                let mut ops = Vec::new();
                for row in rect.start.row..=rect.end.row {
                    for col in rect.start.col..=rect.end.col {
                        let at = CellRef::new(row, col);
                        let wb = session.workbook();
                        let mut style = wb.sheets[sheet]
                            .cells
                            .get(at)
                            .and_then(|c| c.style)
                            .and_then(|id| wb.styles.get(id))
                            .cloned()
                            .unwrap_or_default();
                        style.number_format = (!general).then(|| code.to_owned());
                        let id = session.workbook_mut().intern_style(style);
                        ops.push(EditOperation::SetStyle {
                            sheet,
                            at,
                            style: Some(id),
                        });
                    }
                }
                session
                    .edit(EditOperation::Batch(ops))
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "done": op,
                    "sheet": session.workbook().sheets[sheet].name,
                    "range": rect.to_string(),
                    "format": if general { "General" } else { code },
                }))
            }
            "add_sheet" => {
                refuse_if_structure_locked(session)?;
                let wb = session.workbook();
                let len = wb.sheets.len();
                let name = match opt_str(args, "name")? {
                    Some(n) => n.trim().to_owned(),
                    None => (len + 1..)
                        .map(|n| format!("Sheet{n}"))
                        .find(|n| !wb.sheets.iter().any(|s| s.name.eq_ignore_ascii_case(n)))
                        .unwrap_or_default(),
                };
                check_sheet_name(session, &name, None)?;
                let index = match args.get("position") {
                    None | Some(Value::Null) => len,
                    Some(v) => {
                        let p = v
                            .as_u64()
                            .filter(|p| *p >= 1)
                            .ok_or("position must be an integer from 1")?;
                        usize::try_from(p - 1).unwrap_or(len).min(len)
                    }
                };
                let id = (1000u64..)
                    .map(|k| SheetId(Id::from_parts(0x4147, k)))
                    .find(|id| !wb.sheets.iter().any(|s| s.id == *id))
                    .ok_or("no free sheet id")?;
                let new = Sheet::new(id, name.clone());
                session
                    .edit(EditOperation::InsertSheet {
                        index,
                        sheet: Box::new(new),
                    })
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "done": op, "name": name, "sheets": sheets_json(session) }))
            }
            _ => {
                // rename_sheet
                refuse_if_structure_locked(session)?;
                let current = opt_str(args, "sheet")?
                    .ok_or("rename_sheet needs `sheet`, the current name")?;
                let index = sheet_index(session, Some(current))?;
                let name = str_arg(args, "name")?.trim().to_owned();
                check_sheet_name(session, &name, Some(index))?;
                session
                    .edit(EditOperation::RenameSheet {
                        index,
                        name: name.clone(),
                    })
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "done": op, "name": name, "sheets": sheets_json(session) }))
            }
        }
    }

    // -----------------------------------------------------------------------
    // save
    // -----------------------------------------------------------------------

    fn save(&mut self, args: &Value) -> ToolResult {
        let path_arg = opt_str(args, "path")?;
        let format_arg = opt_str(args, "format")?.map(parse_format).transpose()?;
        let sandbox = self.sandbox.clone();
        let book = self.book(args)?;
        // The original path is re-checked too: a file that was inside a root
        // when opened may since have been replaced by a symlink out of it.
        let target_text = match path_arg {
            Some(p) => p.to_owned(),
            None => book.path.display().to_string(),
        };
        let target = sandbox
            .resolve_for_write(&target_text)
            .map_err(|e| e.to_string())?;
        let by_name = format_of_path(&target);
        let format = match (format_arg, by_name) {
            (Some(f), Some(n)) if f != n => {
                return Err(format!(
                    "format {} contradicts the file name {}; a .{} file must hold {} bytes",
                    f.extension(),
                    target.display(),
                    n.extension(),
                    n.extension()
                ));
            }
            (Some(f), _) => f,
            (None, Some(n)) => n,
            (None, None) if path_arg.is_none() => book.format,
            (None, None) => {
                return Err(format!(
                    "cannot tell the format from {}; pass `format`",
                    target.display()
                ));
            }
        };
        let loss = book.session.loss_writing(format);
        let bytes = book
            .session
            .save_as(format)
            .map_err(|e| format!("nothing was written: {e}"))?;
        write_atomically(&target, &bytes)
            .map_err(|e| format!("could not write {}: {e}", target.display()))?;
        book.path.clone_from(&target);
        book.format = format;
        Ok(json!({
            "path": target.display().to_string(),
            "format": format.extension(),
            "bytes": bytes.len(),
            "loss_report": report_json(&loss),
        }))
    }
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{key}` is required and must be a string"))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

/// `at` for an insert/delete: 1-based row number, or a column letter / 1-based
/// column number. Returned 0-based.
fn line_arg(args: &Value, rows: bool) -> Result<u32, String> {
    let at = args.get("at").ok_or("`at` is required")?;
    let one_based = match at {
        Value::Number(n) => n.as_u64(),
        Value::String(s) if rows => s.trim().parse::<u64>().ok(),
        Value::String(s) => match parse_col(s.trim()) {
            Some(c) => Some(u64::from(c) + 1),
            None => s.trim().parse::<u64>().ok(),
        },
        _ => None,
    };
    let max = if rows { MAX_ROW } else { MAX_COL };
    one_based
        .filter(|n| *n >= 1 && *n - 1 <= u64::from(max))
        .and_then(|n| u32::try_from(n - 1).ok())
        .ok_or_else(|| {
            if rows {
                "`at` must be a row number from 1 to 1048576".to_owned()
            } else {
                "`at` must be a column letter (A..XFD) or number from 1 to 16384".to_owned()
            }
        })
}

fn count_arg(args: &Value) -> Result<u32, String> {
    match args.get("count") {
        None | Some(Value::Null) => Ok(1),
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= 1)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| "`count` must be a positive integer".to_owned()),
    }
}

fn parse_format(text: &str) -> Result<SessionFormat, String> {
    SessionFormat::for_extension(text.trim().trim_start_matches('.'))
        .ok_or_else(|| format!("unknown format {text:?}; one of xlsx, xlsm, ods, csv, tsv, psv"))
}

fn format_of_path(path: &Path) -> Option<SessionFormat> {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(SessionFormat::for_extension)
}

/// The clock `TODAY()`/`NOW()` read: the wall clock in UTC, as a date serial.
/// The SDK takes the clock from its host rather than reading one, and this
/// server is that host.
fn environment_now() -> Environment {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Environment {
        now: since.as_secs_f64() / 86_400.0 + 25_569.0,
        seed: u64::from(since.subsec_nanos()) ^ since.as_secs(),
    }
}

// ---------------------------------------------------------------------------
// Sheets and ranges
// ---------------------------------------------------------------------------

fn sheet_index(session: &WorkbookSession, name: Option<&str>) -> Result<usize, String> {
    let sheets = &session.workbook().sheets;
    if sheets.is_empty() {
        return Err("the workbook has no sheets".to_owned());
    }
    let Some(name) = name else { return Ok(0) };
    sheets
        .iter()
        .position(|s| s.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
            format!("no sheet {name:?}; sheets are: {}", names.join(", "))
        })
}

fn used_range(sheet: &Sheet) -> Option<Rect> {
    let mut it = sheet.cells.iter().map(|(at, _)| at);
    let first = it.next()?;
    let (mut r0, mut c0, mut r1, mut c1) = (first.row, first.col, first.row, first.col);
    for at in it {
        r0 = r0.min(at.row);
        c0 = c0.min(at.col);
        r1 = r1.max(at.row);
        c1 = c1.max(at.col);
    }
    Some(Rect {
        start: CellRef::new(r0, c0),
        end: CellRef::new(r1, c1),
    })
}

/// A range argument tied to a sheet: the sheet index and the rectangle, which
/// is `None` when whole lines were named on a sheet with nothing in them.
fn resolve_range(
    session: &WorkbookSession,
    text: &str,
    sheet_arg: Option<&str>,
) -> Result<(usize, Option<Rect>), String> {
    let (qualified, body) = a1::split_sheet(text)?;
    let sheet = match (qualified.as_deref(), sheet_arg) {
        (Some(q), Some(s)) if !q.eq_ignore_ascii_case(s.trim()) => {
            return Err(format!("the range names sheet {q:?} but `sheet` is {s:?}"));
        }
        (Some(q), _) => sheet_index(session, Some(q))?,
        (None, s) => sheet_index(session, s)?,
    };
    let span = a1::parse_span(body)?;
    let rect = match span {
        Span::Cells(r) => Some(r),
        Span::Cols(c0, c1) => used_range(&session.workbook().sheets[sheet]).and_then(|u| {
            let (a, b) = (c0.max(u.start.col), c1.min(u.end.col));
            (a <= b).then(|| Rect {
                start: CellRef::new(u.start.row, a),
                end: CellRef::new(u.end.row, b),
            })
        }),
        Span::Rows(r0, r1) => used_range(&session.workbook().sheets[sheet]).and_then(|u| {
            let (a, b) = (r0.max(u.start.row), r1.min(u.end.row));
            (a <= b).then(|| Rect {
                start: CellRef::new(a, u.start.col),
                end: CellRef::new(b, u.end.col),
            })
        }),
    };
    Ok((sheet, rect))
}

/// The part of `rect` one call returns — whole rows, at most [`MAX_CELLS`]
/// cells — and the rest of it, if any.
///
/// The rest is a list because a range wider than the budget leaves an
/// L-shape — the end of the first row, and every row below — which no single
/// rectangle names. Saying only the first piece would skip the second
/// silently.
fn clip_to_budget(rect: Rect) -> (Rect, Vec<Rect>) {
    if rect.cells() <= MAX_CELLS {
        return (rect, Vec::new());
    }
    let cols = rect.cols().min(MAX_CELLS);
    let rows = (MAX_CELLS / cols).max(1);
    // Both fit in u32: cols ≤ 16,384 and rows ≤ 10,000.
    let last_col = rect.start.col + (cols as u32) - 1;
    let last_row = rect.start.row + (rows as u32) - 1;
    let returned = Rect {
        start: rect.start,
        end: CellRef::new(last_row, last_col),
    };
    let mut next = Vec::new();
    if last_col < rect.end.col {
        // Wider than the budget (then `rows` is 1): the rest of that row.
        next.push(Rect {
            start: CellRef::new(rect.start.row, last_col + 1),
            end: CellRef::new(last_row, rect.end.col),
        });
    }
    if last_row < rect.end.row {
        next.push(Rect {
            start: CellRef::new(last_row + 1, rect.start.col),
            end: rect.end,
        });
    }
    (returned, next)
}

fn sheets_json(session: &WorkbookSession) -> Value {
    let sheets = &session.workbook().sheets;
    Value::Array(
        sheets
            .iter()
            .enumerate()
            .map(|(index, sheet)| {
                json!({
                    "index": index,
                    "name": sheet.name,
                    "used_range": used_range(sheet).map(|r| r.to_string()),
                    "cells": sheet.cells.iter().count(),
                    "visibility": match sheet.visibility {
                        SheetVisibility::Visible => "visible",
                        SheetVisibility::Hidden => "hidden",
                        SheetVisibility::VeryHidden => "veryHidden",
                    },
                    "protected": sheet.protection.as_ref().is_some_and(|p| p.is_enabled()),
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

fn number_json(n: f64) -> Value {
    serde_json::Number::from_f64(n).map_or_else(|| Value::String(n.to_string()), Value::Number)
}

fn value_json(session: &WorkbookSession, sheet: usize, at: CellRef) -> Value {
    let wb = session.workbook();
    let Some(cell) = wb.sheets.get(sheet).and_then(|s| s.cells.get(at)) else {
        return Value::Null;
    };
    match &cell.value {
        CellValue::Empty => Value::Null,
        CellValue::Number(n) => number_json(*n),
        CellValue::Bool(b) => Value::Bool(*b),
        CellValue::Error(e) => json!({ "error": e.to_string() }),
        CellValue::SharedString(id) | CellValue::InlineString(id) => {
            Value::String(wb.strings.get(*id).unwrap_or_default().to_owned())
        }
    }
}

fn both_json(session: &WorkbookSession, sheet: usize, at: CellRef) -> Value {
    let wb = session.workbook();
    let Some(cell) = wb.sheets.get(sheet).and_then(|s| s.cells.get(at)) else {
        return Value::Null;
    };
    let mut out = Map::new();
    out.insert("value".to_owned(), value_json(session, sheet, at));
    if cell.formula.is_some() {
        out.insert(
            "formula".to_owned(),
            Value::String(session.cell_input(sheet, at)),
        );
    }
    out.insert(
        "display".to_owned(),
        Value::String(session.cell_display(sheet, at)),
    );
    Value::Object(out)
}

fn evaluated_json(value: &Evaluated) -> Value {
    match value {
        Evaluated::Number(n) => number_json(*n),
        Evaluated::Bool(b) => Value::Bool(*b),
        Evaluated::Text(t) => Value::String(t.clone()),
        Evaluated::Error(e) => json!({ "error": e }),
        Evaluated::Array { cols, cells, .. } => {
            let cols = (*cols).max(1);
            let rows: Vec<Value> = cells
                .iter()
                .take(usize::try_from(MAX_CELLS).unwrap_or(usize::MAX))
                .collect::<Vec<_>>()
                .chunks(cols)
                .map(|row| Value::Array(row.iter().map(|v| evaluated_json(v)).collect()))
                .collect();
            json!({ "array": rows })
        }
        // `Empty`, and anything a later SDK adds that this does not know.
        _ => Value::Null,
    }
}

fn report_json(report: &CompatibilityReport) -> Value {
    let entries: Vec<Value> = report
        .entries()
        .into_iter()
        .map(|e| {
            json!({
                "feature": e.feature,
                "model": match e.model {
                    ModelOutcome::Mapped => "mapped",
                    ModelOutcome::Degraded => "degraded",
                    ModelOutcome::Omitted => "omitted",
                },
                "retention": match e.retention {
                    RetentionOutcome::Preserved => "preserved",
                    RetentionOutcome::NotRetained => "not_retained",
                    RetentionOutcome::NotApplicable => "not_applicable",
                },
                "count": e.count,
            })
        })
        .collect();
    json!({ "lossless": report.is_clean(), "entries": entries })
}

// ---------------------------------------------------------------------------
// Typed entry, protection and validation
// ---------------------------------------------------------------------------

/// Whether a cell is locked against edits: its sheet is protected and the
/// cell's style does not unlock it. An empty cell has no style and so is
/// locked, as in Excel.
fn cell_locked(session: &WorkbookSession, sheet: usize, at: CellRef) -> bool {
    let wb = session.workbook();
    let Some(sh) = wb.sheets.get(sheet) else {
        return false;
    };
    if !sh.protection.as_ref().is_some_and(|p| p.is_enabled()) {
        return false;
    }
    sh.cells
        .get(at)
        .and_then(|c| c.style)
        .and_then(|id| wb.styles.get(id))
        .and_then(|st| st.locked)
        .unwrap_or(true)
}

fn refuse_if_protected(
    session: &WorkbookSession,
    sheet: usize,
    action: &str,
) -> Result<(), String> {
    let sh = &session.workbook().sheets[sheet];
    match &sh.protection {
        Some(p) if p.is_enabled() && !p.permits(action) => Err(format!(
            "sheet {:?} is protected and its protection does not allow {action}",
            sh.name
        )),
        _ => Ok(()),
    }
}

fn refuse_if_structure_locked(session: &WorkbookSession) -> Result<(), String> {
    let locked = session
        .workbook()
        .settings
        .protection
        .get("lockStructure")
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if locked {
        return Err(
            "the workbook's structure is protected; sheets cannot be added or renamed".to_owned(),
        );
    }
    Ok(())
}

/// Excel's rules for a sheet name.
fn check_sheet_name(
    session: &WorkbookSession,
    name: &str,
    except: Option<usize>,
) -> Result<(), String> {
    if name.is_empty() {
        return Err("a sheet name cannot be empty".to_owned());
    }
    if name.chars().count() > 31 {
        return Err("a sheet name can be at most 31 characters".to_owned());
    }
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']'))
    {
        return Err(format!("a sheet name cannot contain {c:?}"));
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Err("a sheet name cannot begin or end with an apostrophe".to_owned());
    }
    if name.eq_ignore_ascii_case("History") {
        return Err("\"History\" is reserved by Excel".to_owned());
    }
    let taken = session
        .workbook()
        .sheets
        .iter()
        .enumerate()
        .any(|(i, s)| Some(i) != except && s.name.to_lowercase() == name.to_lowercase());
    if taken {
        return Err(format!("there is already a sheet named {name:?}"));
    }
    Ok(())
}

/// The number typed entry produced, if it produced one.
fn typed_number(op: &EditOperation) -> Option<f64> {
    match op {
        EditOperation::SetValue {
            value: CellValue::Number(n),
            ..
        } => Some(*n),
        EditOperation::SetCell {
            cell:
                Some(Cell {
                    value: CellValue::Number(n),
                    formula: None,
                    ..
                }),
            ..
        } => Some(*n),
        _ => None,
    }
}

/// The edit one input makes, or why it is refused. The `Some` beside an edit
/// is a validation warning the rule's author chose not to make a refusal.
fn typed_op(
    session: &mut WorkbookSession,
    sheet: usize,
    at: CellRef,
    input: &Value,
) -> Result<(EditOperation, Option<String>), String> {
    if cell_locked(session, sheet, at) {
        return Err("the sheet is protected and this cell is locked".to_owned());
    }
    let (text, op) = match input {
        Value::Null => (String::new(), session.input_edit(sheet, at, "")),
        Value::Bool(b) => (
            if *b { "TRUE" } else { "FALSE" }.to_owned(),
            EditOperation::SetValue {
                sheet,
                at,
                value: CellValue::Bool(*b),
            },
        ),
        Value::Number(n) => {
            let n = n
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("not a finite number")?;
            (
                number_text(n),
                EditOperation::SetValue {
                    sheet,
                    at,
                    value: CellValue::Number(n),
                },
            )
        }
        Value::String(s) => {
            let op = session.input_edit(sheet, at, s);
            let trimmed = s.trim();
            if trimmed.starts_with('=') {
                let is_formula = matches!(
                    &op,
                    EditOperation::SetCell { cell: Some(c), .. } if c.formula.is_some()
                );
                if !is_formula {
                    // Typed entry quietly keeps an unparseable formula as
                    // text; Excel refuses it, and so does this.
                    let why = match session.evaluate(sheet, trimmed) {
                        Err(e) => e.to_string(),
                        Ok(_) => "not a formula".to_owned(),
                    };
                    return Err(format!(
                        "{why}. To store it as text, start it with an apostrophe"
                    ));
                }
                // A formula's result is not validated on entry (Excel checks
                // it only on recalculation, through the circle-invalid tool).
                return Ok((op, None));
            }
            (trimmed.to_owned(), op)
        }
        _ => return Err("a cell must be a string, number, boolean or null".to_owned()),
    };
    if text.is_empty() {
        return Ok((op, None));
    }
    let wb = session.workbook();
    let Some(rule) = wb.sheets[sheet]
        .validations
        .iter()
        .find(|v| v.covers(at.row, at.col))
    else {
        return Ok((op, None));
    };
    // A list whose items live in cells is resolved by the editor host, not by
    // the model; nothing here can judge it, so it is not refused on.
    if rule.kind == DvKind::List && rule.values.is_empty() {
        return Ok((op, None));
    }
    if rule.accepts(&text, typed_number(&op)) != Some(false) {
        return Ok((op, None));
    }
    let message = if rule.error_text.is_empty() {
        if rule.kind == DvKind::List {
            format!("must be one of: {}", rule.values.join(", "))
        } else {
            "does not satisfy the cell's data validation rule".to_owned()
        }
    } else {
        rule.error_text.clone()
    };
    match rule.error_style.as_deref() {
        None | Some("stop") => Err(format!("data validation: {message}")),
        Some(_) => Ok((op, Some(format!("data validation: {message}")))),
    }
}

fn number_text(n: f64) -> String {
    format!("{n}")
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Write `bytes` to `target` through a temporary file in the same directory
/// and a rename, so a failed write never leaves a half-written workbook, and
/// a symlink swapped in at `target` after the check is replaced rather than
/// followed.
fn write_atomically(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = target.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    let name = target
        .file_name()
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .to_string_lossy();
    let tmp = dir.join(format!(
        ".{name}.casual-calc-mcp-{}-{}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(a: &str, b: &str) -> Rect {
        Rect {
            start: a1::parse_cell(a).unwrap(),
            end: a1::parse_cell(b).unwrap(),
        }
    }

    /// Whatever a read returns plus whatever it says is left must be the
    /// whole range — truncation stated, never silent, and nothing skipped.
    #[test]
    fn a_truncated_read_names_exactly_what_it_left_out() {
        for r in [
            rect("A1", "C3"),
            rect("A1", "Z1000"),
            rect("B2", "B20000"),
            rect("A1", "XFD1"),
            rect("A1", "XFD3"),
        ] {
            let (returned, next) = clip_to_budget(r);
            assert!(returned.cells() <= MAX_CELLS, "{r}");
            let rest: u64 = next.iter().map(Rect::cells).sum();
            assert_eq!(returned.cells() + rest, r.cells(), "{r} lost cells");
        }
    }

    // -- the tools against a real file -------------------------------------

    use casual_calc_model::{DataValidation, DvOperator, SheetProtection, Style};
    use casual_calc_sdk::Cell;

    struct Fixture {
        tools: Tools,
        wb: String,
        dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Fixture {
        /// A one-sheet workbook shaped by `build`, saved as `.xlsx` and opened
        /// through the tool, so whatever the test relies on has survived a file.
        fn new(build: impl FnOnce(&mut WorkbookSession)) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "casual-calc-mcp-tools-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mut s = WorkbookSession::with_sheet();
            build(&mut s);
            std::fs::write(dir.join("book.xlsx"), s.save().unwrap()).unwrap();
            let mut tools = Tools::new(Sandbox::new(std::slice::from_ref(&dir)).unwrap());
            let opened = done(tools.call("open_workbook", &json!({ "path": "book.xlsx" })));
            let wb = opened.unwrap()["handle"].as_str().unwrap().to_owned();
            Self { tools, wb, dir }
        }

        fn call(&mut self, name: &str, mut args: Value) -> Result<Value, String> {
            args["handle"] = json!(self.wb);
            done(self.tools.call(name, &args))
        }
    }

    fn done(call: Call) -> Result<Value, String> {
        match call {
            Call::Done(r) => r,
            Call::Unknown => panic!("unknown tool"),
        }
    }

    fn type_into(s: &mut WorkbookSession, cell: &str, text: &str) {
        let op = s.input_edit(0, a1::parse_cell(cell).unwrap(), text);
        s.edit(op).unwrap();
    }

    #[test]
    fn a_protected_sheet_refuses_locked_cells_and_structural_edits() {
        let mut f = Fixture::new(|s| {
            type_into(s, "A1", "1");
            let unlocked = s.workbook_mut().intern_style(Style {
                locked: Some(false),
                ..Style::default()
            });
            let mut cell = Cell::value(CellValue::Empty);
            cell.style = Some(unlocked);
            s.workbook_mut().sheets[0]
                .cells
                .set(CellRef::new(0, 1), cell);
            s.workbook_mut().sheets[0].protection = Some(SheetProtection::enabled());
        });
        let r = f
            .call("write_cells", json!({ "anchor": "A1", "values": [[5, 6]] }))
            .unwrap();
        assert_eq!(r["written"], 1, "{r}");
        assert_eq!(r["refused"][0]["cell"], "A1");
        assert!(
            r["refused"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("locked")
        );
        assert_eq!(r["values"], json!([[1.0, 6.0]]));
        let e = f
            .call("run", json!({ "op": "insert_rows", "at": 1 }))
            .unwrap_err();
        assert!(e.contains("protected"), "{e}");
        let e = f
            .call(
                "run",
                json!({ "op": "set_number_format", "range": "B1", "format": "0%" }),
            )
            .unwrap_err();
        assert!(e.contains("formatCells"), "{e}");
    }

    #[test]
    fn data_validation_refuses_a_stop_rule_and_warns_for_the_others() {
        let mut f = Fixture::new(|s| {
            let rule = |col: u32, style: Option<&str>| DataValidation {
                range: casual_calc_model::CellRange::new(
                    CellRef::new(0, col),
                    CellRef::new(9, col),
                ),
                values: Vec::new(),
                kind: DvKind::Whole,
                operator: DvOperator::Between,
                formula1: "1".to_owned(),
                formula2: "10".to_owned(),
                allow_blank: true,
                error_style: style.map(str::to_owned),
                hide_dropdown: false,
                ime_mode: None,
                error_title: String::new(),
                error_text: String::new(),
                prompt_title: String::new(),
                prompt_text: String::new(),
            };
            let sheet = &mut s.workbook_mut().sheets[0];
            sheet.validations.push(rule(0, None));
            sheet.validations.push(rule(1, Some("warning")));
            // Keep the sheet non-empty so it saves with its rules.
            type_into(s, "D1", "x");
        });
        let r = f
            .call(
                "write_cells",
                json!({ "anchor": "A1", "values": [["11", 99], [5, null], ["50%", null]] }),
            )
            .unwrap();
        let refused: Vec<&str> = r["refused"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["cell"].as_str().unwrap())
            .collect();
        assert_eq!(refused, ["A1", "A3"], "{r}");
        assert_eq!(r["warnings"][0]["cell"], "B1", "{r}");
        assert_eq!(
            r["values"],
            json!([[null, 99.0], [5.0, null], [null, null]])
        );
    }

    #[test]
    fn a_write_past_the_grid_is_refused_cell_by_cell() {
        let mut f = Fixture::new(|_| {});
        let r = f
            .call(
                "write_cells",
                json!({ "anchor": "XFD1", "values": [[1, 2]] }),
            )
            .unwrap();
        assert_eq!(r["written"], 1, "{r}");
        assert_eq!(r["refused"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn sheets_are_added_and_renamed_under_excel_rules() {
        let mut f = Fixture::new(|s| type_into(s, "A1", "7"));
        f.call("run", json!({ "op": "add_sheet", "name": "Data" }))
            .unwrap();
        let e = f
            .call("run", json!({ "op": "add_sheet", "name": "data" }))
            .unwrap_err();
        assert!(e.contains("already"), "{e}");
        let e = f
            .call(
                "run",
                json!({ "op": "rename_sheet", "sheet": "Data", "name": "a/b" }),
            )
            .unwrap_err();
        assert!(e.contains('/'), "{e}");
        f.call(
            "run",
            json!({ "op": "rename_sheet", "sheet": "Sheet1", "name": "Q1 Sales" }),
        )
        .unwrap();
        let r = f
            .call(
                "write_cells",
                json!({ "anchor": "Data!A1", "values": [["='Q1 Sales'!A1*2"]] }),
            )
            .unwrap();
        assert_eq!(r["values"], json!([[14.0]]), "{r}");
        let r = f
            .call("read_range", json!({ "range": "'Q1 Sales'!A:A" }))
            .unwrap();
        assert_eq!(r["range"], "A1");
        assert_eq!(r["rows"], json!([[7.0]]));
    }

    #[test]
    fn number_formats_change_what_the_grid_shows() {
        let mut f = Fixture::new(|s| type_into(s, "A1", "0.5"));
        f.call(
            "run",
            json!({ "op": "set_number_format", "range": "A1", "format": "0.00%" }),
        )
        .unwrap();
        let r = f
            .call("read_range", json!({ "range": "A1", "mode": "both" }))
            .unwrap();
        assert_eq!(r["rows"][0][0]["display"], "50.00%");
        assert_eq!(r["rows"][0][0]["value"], 0.5);
    }

    /// Excel: "can't insert new cells because it would push non-empty cells
    /// off the end of the worksheet". Pushing them off is silent data loss.
    #[test]
    fn an_insert_that_would_push_data_off_the_grid_is_refused() {
        let mut f = Fixture::new(|s| {
            type_into(s, "A1", "keep");
            type_into(s, "C1", "keep too");
        });
        let e = f
            .call(
                "run",
                json!({ "op": "insert_rows", "at": 1, "count": 1_048_576 }),
            )
            .unwrap_err();
        assert!(e.contains("off the end"), "{e}");
        let e = f
            .call(
                "run",
                json!({ "op": "insert_columns", "at": "B", "count": 16_383 }),
            )
            .unwrap_err();
        assert!(e.contains("off the end"), "{e}");
        // Up to the edge is fine.
        f.call(
            "run",
            json!({ "op": "insert_columns", "at": "B", "count": 16_381 }),
        )
        .unwrap();
        let r = f.call("read_range", json!({ "range": "A1:A1" })).unwrap();
        assert_eq!(r["rows"], json!([["keep"]]));
        let r = f.call("read_range", json!({ "range": "XFD1" })).unwrap();
        assert_eq!(r["rows"], json!([["keep too"]]));
    }

    #[test]
    fn a_locked_workbook_structure_refuses_sheet_changes() {
        let mut f = Fixture::new(|s| {
            type_into(s, "A1", "1");
            s.workbook_mut()
                .settings
                .protection
                .insert("lockStructure".to_owned(), "1".to_owned());
        });
        let e = f.call("run", json!({ "op": "add_sheet" })).unwrap_err();
        assert!(e.contains("structure"), "{e}");
    }
}
