//! Structured data over the grid: validation, conditional formats,
//! autofilter and tables.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// The dropdown values for the validation covering `(row, col)` as a JSON array,
/// or `null` if the cell has no list validation.
#[wasm_bindgen]
pub fn session_validation_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "null".to_owned();
        };
        // Only a list rule has anything to pick from. A number or date rule
        // returned an empty array, which the host read as "there is a dropdown"
        // — every JS array is truthy — so a whole-number cell grew a chevron
        // that opened onto nothing.
        match sh
            .validations
            .iter()
            .find(|v| v.covers(row, col))
            .filter(|v| v.kind == casual_calc_model::DvKind::List && !v.values.is_empty())
            // `showDropDown="1"` *hides* the in-cell list, as the schema
            // defines it. A file that asked for a typed-only list was still
            // getting a chevron.
            .filter(|v| !v.hide_dropdown)
        {
            Some(v) => {
                let items: Vec<String> = v.values.iter().map(|x| json_string(x)).collect();
                format!("[{}]", items.join(","))
            }
            None => "null".to_owned(),
        }
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// Why `input` is not allowed in `(row, col)`, or an empty string if it is.
///
/// A dropdown that accepts anything typed over it is not a validation — it is a
/// suggestion. The host calls this before committing an edit and refuses the
/// commit with the returned message, which is how Excel behaves (and, like
/// Excel, only for typed entry: fill and paste are not gated).
///
/// An empty input always passes: clearing a cell is not entering a bad value.
///
/// Returns `""` when the value is allowed, otherwise JSON
/// `{"style":"stop"|"warning"|"information","title":…,"text":…}`.
///
/// The style matters and used to be dropped: only `stop` refuses the entry.
/// `warning` asks whether to keep it and `information` merely says so — turning
/// either into a hard block is a different rule from the one the author wrote,
/// and there is no way for the user to get past it.
#[wasm_bindgen]
pub fn session_validation_error(sheet: usize, row: u32, col: u32, input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let out = with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return String::new();
        };
        let Some(rule) = sh.validations.iter().find(|v| v.covers(row, col)) else {
            return String::new();
        };
        // The model decides; this only phrases the refusal. `None` means the
        // rule needs the formula engine, so nothing is blocked on it.
        let number = trimmed.parse::<f64>().ok();
        if rule.accepts(trimmed, number) != Some(false) {
            return String::new();
        }
        // Author-set wording always wins: they know what the rule is for.
        if !rule.error_text.is_empty() {
            return rule.error_text.clone();
        }
        if rule.kind == casual_calc_model::DvKind::List {
            let shown: Vec<&str> = rule.values.iter().take(6).map(String::as_str).collect();
            let ellipsis = if rule.values.len() > shown.len() {
                ", …"
            } else {
                ""
            };
            return format!("must be one of: {}{ellipsis}", shown.join(", "));
        }
        let what = match rule.kind {
            casual_calc_model::DvKind::Whole => "a whole number",
            casual_calc_model::DvKind::Decimal => "a number",
            casual_calc_model::DvKind::Date => "a date",
            casual_calc_model::DvKind::Time => "a time",
            casual_calc_model::DvKind::TextLength => "text of an allowed length",
            _ => "a permitted value",
        };
        let bound = match rule.operator {
            casual_calc_model::DvOperator::Between => {
                format!(" between {} and {}", rule.formula1, rule.formula2)
            }
            casual_calc_model::DvOperator::NotBetween => {
                format!(" outside {} to {}", rule.formula1, rule.formula2)
            }
            casual_calc_model::DvOperator::Equal => format!(" equal to {}", rule.formula1),
            casual_calc_model::DvOperator::NotEqual => format!(" not equal to {}", rule.formula1),
            casual_calc_model::DvOperator::GreaterThan => {
                format!(" greater than {}", rule.formula1)
            }
            casual_calc_model::DvOperator::LessThan => format!(" less than {}", rule.formula1),
            casual_calc_model::DvOperator::GreaterThanOrEqual => {
                format!(" at least {}", rule.formula1)
            }
            casual_calc_model::DvOperator::LessThanOrEqual => {
                format!(" at most {}", rule.formula1)
            }
        };
        format!(
            "must be {what}{}",
            if rule.formula1.is_empty() {
                String::new()
            } else {
                bound
            }
        )
    })
    .unwrap_or_default();
    if out.is_empty() {
        return out;
    }
    let (style, title) = with_session(|s| {
        s.workbook()
            .sheets
            .get(sheet)
            .and_then(|sh| sh.validations.iter().find(|v| v.covers(row, col)))
            .map(|r| {
                (
                    r.error_style.clone().unwrap_or_else(|| "stop".to_owned()),
                    r.error_title.clone(),
                )
            })
    })
    .flatten()
    .unwrap_or_else(|| ("stop".to_owned(), String::new()));
    format!(
        "{{\"style\":{},\"title\":{},\"text\":{}}}",
        json_string(&style),
        json_string(&title),
        json_string(&out)
    )
}

/// The input hint on a cell — Excel's "Input Message" — as JSON
/// `{"title":…,"text":…}`, or `""` where the cell has none.
///
/// Shown when the cell is selected, which is the whole point of it: a rule that
/// only speaks up after you have typed something wrong explains the constraint
/// too late.
#[wasm_bindgen]
pub fn session_validation_prompt(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let rule = s
            .workbook()
            .sheets
            .get(sheet)?
            .validations
            .iter()
            .find(|v| v.covers(row, col))?;
        if rule.prompt_title.is_empty() && rule.prompt_text.is_empty() {
            return None;
        }
        Some(format!(
            "{{\"title\":{},\"text\":{}}}",
            json_string(&rule.prompt_title),
            json_string(&rule.prompt_text)
        ))
    })
    .flatten()
    .unwrap_or_default()
}

/// The wording and flags on the rule covering a cell, as JSON, or `""` when the
/// cell has no rule. The panel loads this so editing a rule keeps the author's
/// wording instead of blanking it on the next Apply.
#[wasm_bindgen]
pub fn session_validation_messages(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let r = s
            .workbook()
            .sheets
            .get(sheet)?
            .validations
            .iter()
            .find(|v| v.covers(row, col))?;
        Some(format!(
            "{{\"style\":{},\"errorTitle\":{},\"errorText\":{},\
             \"promptTitle\":{},\"promptText\":{},\"hideDropdown\":{}}}",
            json_string(r.error_style.as_deref().unwrap_or("stop")),
            json_string(&r.error_title),
            json_string(&r.error_text),
            json_string(&r.prompt_title),
            json_string(&r.prompt_text),
            r.hide_dropdown,
        ))
    })
    .flatten()
    .unwrap_or_default()
}

/// Set the messages and the dropdown flag on the rules covering a range,
/// leaving the rule itself alone.
///
/// Separate from `session_set_validation` because they are separate decisions:
/// Excel has an "Input Message" and an "Error Alert" tab precisely so wording
/// can be changed without redefining what is allowed.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_validation_messages(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    style: &str,
    titles: Vec<String>,
    hide_dropdown: bool,
) -> Result<(), JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    // `titles` is [error title, error text, prompt title, prompt text] — four
    // strings of the same kind, which read worse as four positional arguments
    // than as the list they are.
    let get = |i: usize| titles.get(i).cloned().unwrap_or_default();
    let (et, ex, pt, px) = (get(0), get(1), get(2), get(3));
    let style = style.to_owned();
    edit_sheet_metadata(sheet, move |_, data| {
        for v in data.validations.iter_mut() {
            if v.range.start.row <= rr1
                && v.range.end.row >= rr0
                && v.range.start.col <= cc1
                && v.range.end.col >= cc0
            {
                // `stop` is the schema default, so writing it back as `None`
                // keeps an untouched file byte-identical.
                v.error_style = (!style.is_empty() && style != "stop").then(|| style.clone());
                v.error_title = et.clone();
                v.error_text = ex.clone();
                v.prompt_title = pt.clone();
                v.prompt_text = px.clone();
                v.hide_dropdown = hide_dropdown;
            }
        }
    })
}

/// Set a non-list validation over a range: `kind` and `op` are the OOXML tokens,
/// `f1`/`f2` the operands, plus the author's own message wording.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_validation(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
    op: &str,
    f1: &str,
    f2: &str,
    allow_blank: bool,
    error_text: &str,
) -> Result<(), JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    edit_sheet_metadata(sheet, move |_, data| {
        // Replace whatever covered this block, as the list setter does.
        data.validations.retain(|v| {
            !(v.range.start.row <= rr1
                && v.range.end.row >= rr0
                && v.range.start.col <= cc1
                && v.range.end.col >= cc0)
        });
        let range = CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1));
        data.validations.push(casual_calc_model::DataValidation {
            kind: casual_calc_model::DvKind::from_ooxml(kind),
            operator: casual_calc_model::DvOperator::from_ooxml(op),
            formula1: f1.trim().to_owned(),
            formula2: f2.trim().to_owned(),
            allow_blank,
            error_text: error_text.trim().to_owned(),
            ..casual_calc_model::DataValidation::none(range)
        });
    })
}

/// Remove any validation intersecting a range.
#[wasm_bindgen]
pub fn session_clear_validation(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        data.validations.retain(|v| {
            !(v.range.start.row <= r1
                && v.range.end.row >= r0
                && v.range.start.col <= c1
                && v.range.end.col >= c0)
        });
    })
}

/// Add a highlight-cells conditional-format rule over a range. `kind` is one of
/// `gt`/`lt`/`eq`/`between`/`contains`; `a`/`b` are numeric operands (b only for
/// `between`), `text` the substring for `contains`, `fill` the `RRGGBB` color.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_add_cf(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    kind: &str,
    a: f64,
    b: f64,
    text: &str,
    fill: &str,
) -> Result<(), JsError> {
    let rule = match kind {
        "gt" => CfRule::GreaterThan(a),
        "lt" => CfRule::LessThan(a),
        "eq" => CfRule::EqualTo(a),
        "between" => CfRule::Between(a.min(b), a.max(b)),
        "contains" => CfRule::TextContains(text.to_owned()),
        // Range-relative kinds take their colours through `text` as a
        // comma-separated list (low → high), since they need two or three and
        // the single `fill` slot cannot carry them.
        "colorscale" => {
            let colors: Vec<String> = text
                .split(',')
                .map(|c| c.trim().trim_start_matches('#').to_ascii_uppercase())
                .filter(|c| c.len() == 6)
                .collect();
            if colors.len() < 2 {
                return Err(JsError::new("a colour scale needs at least two colours"));
            }
            CfRule::ColorScale(colors)
        }
        "databar" => CfRule::DataBar(text.trim().trim_start_matches('#').to_ascii_uppercase()),
        // Ranked / statistical kinds: the operand `a` is the rank where one
        // applies, and a rank of zero would select nothing.
        "top" | "bottom" | "toppct" | "bottompct" => CfRule::Top10 {
            rank: (a as u32).max(1),
            bottom: kind.starts_with("bottom"),
            percent: kind.ends_with("pct"),
        },
        "above" | "below" => CfRule::AboveAverage {
            below: kind == "below",
            equal: false,
        },
        "duplicate" => CfRule::DuplicateValues { unique: false },
        "unique" => CfRule::DuplicateValues { unique: true },
        _ => return Err(JsError::new("unknown conditional-format rule")),
    };
    let fill = fill.trim().trim_start_matches('#').to_ascii_uppercase();
    edit_sheet_metadata(sheet, move |_, data| {
        let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
        // New rules go last in priority, so they do not silently outrank the
        // ones already there.
        let next = data
            .conditional_formats
            .iter()
            .map(|c| c.priority)
            .max()
            .unwrap_or(0)
            + 1;
        let mut cf = ConditionalFormat::new(
            CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1)),
            rule,
            fill,
        );
        cf.priority = next;
        data.conditional_formats.push(cf);
    })
}

/// Remove every conditional-format rule intersecting a range.
#[wasm_bindgen]
pub fn session_clear_cf(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        data.conditional_formats.retain(|cf| {
            !(cf.range.start.row <= r1
                && cf.range.end.row >= r0
                && cf.range.start.col <= c1
                && cf.range.end.col >= c0)
        });
    })
}

/// Set (or, with empty text, remove) a cell's comment. Replaces the whole
/// thread, so any replies go with it — this is the "edit the note" path.
///
/// `author` and `created` may be empty, which leaves a plain note. `created` is
/// passed in as an ISO 8601 string rather than read from a clock here so the
/// core stays deterministic: the same sequence of edits produces the same bytes.
#[wasm_bindgen]
pub fn session_set_comment(
    sheet: usize,
    row: u32,
    col: u32,
    text: &str,
    author: &str,
    created: &str,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        // Editing keeps the replies that were already on the thread; only
        // an empty text (a delete) drops them.
        let existing = data
            .comments
            .iter()
            .position(|c| c.at.row == row && c.at.col == col);
        let text = text.trim();
        if text.is_empty() {
            if let Some(i) = existing {
                data.comments.remove(i);
            }
            return;
        }
        let mut thread = match existing {
            Some(i) => data.comments.remove(i),
            None => CellComment::note(CellRef::new(row, col), "", None),
        };
        thread.text = text.to_owned();
        if !author.is_empty() {
            thread.author = Some(author.to_owned());
        }
        if !created.is_empty() {
            thread.created = Some(created.to_owned());
        }
        data.comments.push(thread);
    })
}

/// Create a table over a range — Excel's Ctrl+T.
///
/// The header row's cells become the column names, because a structured
/// reference resolves by name: `Sales[Amount]` finds its column through the
/// header text, so a table whose columns disagree with their headers has
/// formulas pointing at nothing. Empty or duplicate headers are filled in, for
/// the same reason.
#[wasm_bindgen]
pub fn session_create_table(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    name: &str,
    has_headers: bool,
) -> Result<String, JsError> {
    let (rr0, cc0, rr1, cc1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
    // A name must be unique across the workbook: structured references are
    // resolved by name alone, so two tables sharing one makes every reference
    // to it ambiguous.
    let taken: Vec<String> = with_session(|s| {
        s.workbook()
            .sheets
            .iter()
            .flat_map(|sh| sh.tables.iter().map(|t| t.name.to_ascii_lowercase()))
            .collect()
    })
    .unwrap_or_default();
    let base = {
        let trimmed = name.trim();
        if trimmed.is_empty() { "Table" } else { trimmed }
    };
    let mut final_name = base.to_owned();
    let mut n = 1;
    while taken.contains(&final_name.to_ascii_lowercase()) {
        n += 1;
        final_name = format!("{base}{n}");
    }

    // Column names come from the header cells when there is a header row.
    let headers: Vec<String> = with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return Vec::new();
        };
        (cc0..=cc1)
            .map(|c| {
                if !has_headers {
                    return String::new();
                }
                sh.cells
                    .get(CellRef::new(rr0, c))
                    .map(|cell| value_text(s.workbook(), &cell.value))
                    .unwrap_or_default()
            })
            .collect()
    })
    .unwrap_or_default();

    let mut names: Vec<String> = Vec::new();
    for (i, header) in headers.iter().enumerate() {
        let mut candidate = header.trim().to_owned();
        if candidate.is_empty() {
            candidate = format!("Column{}", i + 1);
        }
        // Duplicates get a suffix rather than being left to collide: a
        // reference to a duplicated name would resolve to whichever came first,
        // silently reading the wrong column.
        let mut unique = candidate.clone();
        let mut k = 1;
        while names
            .iter()
            .any(|n: &String| n.eq_ignore_ascii_case(&unique))
        {
            k += 1;
            unique = format!("{candidate}{k}");
        }
        names.push(unique);
    }

    let id = with_session(|s| {
        s.workbook()
            .sheets
            .iter()
            .flat_map(|sh| sh.tables.iter().map(|t| t.id))
            .max()
            .unwrap_or(0)
            + 1
    })
    .unwrap_or(1);

    let created = final_name.clone();
    let columns: Vec<casual_calc_model::TableColumn> = names
        .into_iter()
        .enumerate()
        .map(|(i, n)| casual_calc_model::TableColumn {
            id: i as u32 + 1,
            name: n,
            totals_row_function: None,
            totals_row_label: None,
            calculated_column_formula: None,
            totals_row_formula: None,
        })
        .collect();
    edit_sheet_metadata(sheet, move |_, data| {
        data.tables.push(Table {
            id,
            name: final_name.clone(),
            display_name: final_name,
            range: CellRange::new(CellRef::new(rr0, cc0), CellRef::new(rr1, cc1)),
            header_row_count: u32::from(has_headers),
            totals_row_count: 0,
            columns,
            // Excel turns the filter buttons on with the table; without them
            // the header row looks like an ordinary row that happens to be
            // shaded.
            auto_filter: Some(AutoFilter::new(CellRange::new(
                CellRef::new(rr0, cc0),
                CellRef::new(rr1, cc1),
            ))),
            style: [
                ("name".to_owned(), "TableStyleMedium2".to_owned()),
                ("showRowStripes".to_owned(), "1".to_owned()),
            ]
            .into_iter()
            .collect(),
            attrs: Default::default(),
        });
    })?;
    Ok(created)
}

/// Remove the table covering a cell, leaving its values in place — Excel's
/// "Convert to Range".
#[wasm_bindgen]
pub fn session_remove_table(sheet: usize, row: u32, col: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        data.tables.retain(|t| {
            !(row >= t.range.start.row
                && row <= t.range.end.row
                && col >= t.range.start.col
                && col <= t.range.end.col)
        });
    })
}

/// The `SUBTOTAL` function number for a `totalsRowFunction` name.
///
/// The 10x codes ignore rows the filter has hidden, which is the whole point of
/// a table total: filter to one region and the total follows. Excel writes the
/// same codes.
pub(crate) fn totals_subtotal_code(func: &str) -> Option<u32> {
    Some(match func {
        "average" => 101,
        "count" => 103,
        "countNums" => 102,
        "max" => 104,
        "min" => 105,
        "stdDev" => 107,
        "sum" => 109,
        "var" => 110,
        _ => return None,
    })
}

/// Set a column's totals-row function, writing the formula the choice means.
///
/// Excel stores both: `totalsRowFunction="sum"` on the column *and* a real
/// `SUBTOTAL(109, Table[Column])` in the cell. Recording only the attribute —
/// which is all the model did — leaves the totals row blank on screen and in
/// every other reader; writing only the formula loses the choice on save. The
/// two go together, in one undo step, or an undo leaves them disagreeing.
///
/// `func` is an OOXML name (`sum`, `average`, `count`, `countNums`, `max`,
/// `min`, `stdDev`, `var`) or empty to clear the cell back to nothing.
#[wasm_bindgen]
pub fn session_set_totals_function(
    sheet: usize,
    row: u32,
    col: u32,
    func: &str,
) -> Result<(), JsError> {
    let func = func.to_owned();
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
            return Ok(());
        };
        let Some(t) = sh.tables.iter().find(|t| {
            row >= t.range.start.row
                && row <= t.range.end.row
                && col >= t.range.start.col
                && col <= t.range.end.col
        }) else {
            return Ok(());
        };
        if t.totals_row_count == 0 {
            return Err(JsError::new("this table has no totals row"));
        }
        let Some(index) = (col.checked_sub(t.range.start.col)).map(|i| i as usize) else {
            return Ok(());
        };
        let Some(column) = t.columns.get(index) else {
            return Ok(());
        };
        let at = CellRef::new(t.range.end.row, col);
        // The structured reference, not an A1 range: inserting a row into the
        // table has to widen the total, and only the name does that.
        let text = match totals_subtotal_code(&func) {
            Some(code) => format!("=SUBTOTAL({code},{}[{}])", t.name, column.name),
            None => String::new(),
        };

        let mut data = SheetMetadata::capture(&sh);
        if let Some(t) = table_at_mut(&mut data.tables, row, col)
            && let Some(c) = t.columns.get_mut(index)
        {
            c.totals_row_function = (!func.is_empty()).then(|| func.clone());
            // A label and a function are alternatives on the same cell: Excel
            // writes one or the other, never both.
            if !func.is_empty() {
                c.totals_row_label = None;
            }
        }
        let cell_op = session.input_edit(sheet, at, &text);
        session
            .edit(EditOperation::Batch(vec![
                EditOperation::set_sheet_metadata(sheet, data),
                cell_op,
            ]))
            .map_err(js)
    })
}

/// The totals-row function on each column of the table under a cell, as a JSON
/// array of names (empty string where a column has none).
#[wasm_bindgen]
pub fn session_totals_functions(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(t) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.tables.iter().find(|t| {
                row >= t.range.start.row
                    && row <= t.range.end.row
                    && col >= t.range.start.col
                    && col <= t.range.end.col
            })
        }) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = t
            .columns
            .iter()
            .map(|c| json_string(c.totals_row_function.as_deref().unwrap_or_default()))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Rename the table under a cell.
///
/// A structured reference resolves by name alone, so the new name has to be
/// unique across the workbook or `Sales[Amount]` starts pointing at whichever
/// table the resolver reaches first. A clash is rejected rather than silently
/// suffixed: the user typed a specific name and deserves to be told.
#[wasm_bindgen]
pub fn session_rename_table(sheet: usize, row: u32, col: u32, name: &str) -> Result<(), JsError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(JsError::new("a table needs a name"));
    }
    // Excel's rule: a name is an identifier, not a label — no spaces, and it
    // cannot look like a cell reference.
    if name.chars().next().is_some_and(|c| c.is_ascii_digit())
        || name
            .chars()
            .any(|c| !(c.is_alphanumeric() || c == '_' || c == '.'))
    {
        return Err(JsError::new(
            "a table name must start with a letter and hold only letters, digits, _ or .",
        ));
    }
    let clash = with_session(|s| {
        s.workbook().sheets.iter().enumerate().any(|(i, sh)| {
            sh.tables.iter().any(|t| {
                t.name.eq_ignore_ascii_case(&name)
                    && !(i == sheet
                        && row >= t.range.start.row
                        && row <= t.range.end.row
                        && col >= t.range.start.col
                        && col <= t.range.end.col)
            })
        })
    })
    .unwrap_or(false);
    if clash {
        return Err(JsError::new("another table already has that name"));
    }
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            t.name = name.clone();
            t.display_name = name.clone();
        }
    })
}

/// Set the style name and banding flags on the table under a cell.
///
/// The name is what every colour is derived from — Excel stores no fills for a
/// table, only this name — so changing it is what restyles the table.
///
/// `flags` is a bitmask: 1 banded rows, 2 banded columns, 4 emphasise the first
/// column, 8 emphasise the last. One argument rather than four booleans so the
/// whole change stays a single undo step.
#[wasm_bindgen]
pub fn session_set_table_style(
    sheet: usize,
    row: u32,
    col: u32,
    style: &str,
    flags: u32,
) -> Result<(), JsError> {
    let style = style.to_owned();
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            for (bit, key) in [
                (1, "showRowStripes"),
                (2, "showColumnStripes"),
                (4, "showFirstColumn"),
                (8, "showLastColumn"),
            ] {
                t.style
                    .insert(key.to_owned(), u8::from(flags & bit != 0).to_string());
            }
            if style.is_empty() {
                t.style.remove("name");
            } else {
                t.style.insert("name".to_owned(), style.clone());
            }
        }
    })
}

/// Turn a table's header row on or off.
///
/// The range does not move: Excel's "Header Row" checkbox decides whether the
/// table's first row is read as headers, not where the table sits. Shifting the
/// range here would either swallow a row of data or leave one stranded outside
/// the table.
#[wasm_bindgen]
pub fn session_set_table_headers(
    sheet: usize,
    row: u32,
    col: u32,
    on: bool,
) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            t.header_row_count = u32::from(on);
        }
    })
}

/// The colours a style name resolves to, as JSON — what the style picker
/// paints its swatches with, so the preview and the grid cannot disagree.
#[wasm_bindgen]
pub fn session_table_style_preview(style: &str) -> String {
    with_session(|s| {
        let c = table_style_colors(s.workbook(), style);
        format!(
            "{{\"headerFill\":{},\"headerText\":{},\"bodyFill\":{},\"bandFill\":{},\"border\":{}}}",
            json_string(&c.header_fill),
            json_string(&c.header_text),
            json_string(&c.body_fill),
            json_string(&c.band_fill),
            json_string(&c.border),
        )
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// The table covering a cell, mutably. Every table command needs this same
/// lookup, and writing it out each time is how two of them drifted apart.
pub(crate) fn table_at_mut(tables: &mut [Table], row: u32, col: u32) -> Option<&mut Table> {
    tables.iter_mut().find(|t| {
        row >= t.range.start.row
            && row <= t.range.end.row
            && col >= t.range.start.col
            && col <= t.range.end.col
    })
}

/// Turn a table's totals row on or off, growing or shrinking its range.
#[wasm_bindgen]
pub fn session_table_totals(sheet: usize, row: u32, col: u32, on: bool) -> Result<(), JsError> {
    SESSION.with(|cell| {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().ok_or_else(|| JsError::new("no session"))?;
        let Some(sh) = session.workbook().sheets.get(sheet).cloned() else {
            return Ok(());
        };
        let Some(t) = sh.tables.iter().find(|t| {
            row >= t.range.start.row
                && row <= t.range.end.row
                && col >= t.range.start.col
                && col <= t.range.end.col
        }) else {
            return Ok(());
        };
        if (t.totals_row_count > 0) == on {
            return Ok(());
        }
        let first_col = t.range.start.col;
        let last_col = t.range.end.col;
        // Turning it on adds a row below; turning it off gives back the one it
        // occupied, so the cells to write are on different rows in each case.
        let totals_row = if on {
            t.range.end.row + 1
        } else {
            t.range.end.row
        };

        let mut data = SheetMetadata::capture(&sh);
        if let Some(t) = table_at_mut(&mut data.tables, row, col) {
            t.totals_row_count = u32::from(on);
            // The totals row is *inside* the table's range, so switching it
            // must move the bottom edge — leaving the range alone would make
            // the last data row read as the totals row.
            if on {
                t.range.end.row += 1;
            } else {
                t.range.end.row = t.range.end.row.saturating_sub(1);
            }
            if let Some(c) = t.columns.first_mut() {
                // Excel labels the first column "Total" and leaves the rest for
                // the user to choose a function for.
                c.totals_row_label = on.then(|| "Total".to_owned());
            }
            if !on {
                for c in t.columns.iter_mut() {
                    c.totals_row_function = None;
                    c.totals_row_label = None;
                }
            }
        }

        let mut ops = vec![EditOperation::set_sheet_metadata(sheet, data)];
        // Turning the row off has to clear what it held: the range shrinks but
        // the cells do not move, so a stale "Total" would be left sitting under
        // the table looking like data.
        for c in first_col..=last_col {
            let at = CellRef::new(totals_row, c);
            let text = if on && c == first_col { "Total" } else { "" };
            ops.push(session.input_edit(sheet, at, text));
        }
        session.edit(EditOperation::Batch(ops)).map_err(js)
    })
}

/// Grow the table that a newly-typed cell sits directly below or beside.
///
/// Typing in the row under a table extends it, which is the behaviour that
/// makes a table worth having: the range, the banding and every structured
/// reference follow the data instead of needing to be re-pointed by hand.
///
/// A no-op unless the cell is exactly one row below, or one column right of,
/// a table — growing on anything further away would swallow unrelated data.
#[wasm_bindgen]
pub fn session_table_autoexpand(sheet: usize, row: u32, col: u32) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        for table in data.tables.iter_mut() {
            let bottom = table.range.end.row;
            let within_cols = col >= table.range.start.col && col <= table.range.end.col;
            let within_rows = row >= table.range.start.row && row <= table.range.end.row;
            // A totals row sits at the bottom, so a new data row goes *above*
            // it — growing past it would leave the totals stranded mid-table.
            if within_cols && table.totals_row_count == 0 && row == bottom + 1 {
                table.range.end.row = row;
                // Widen the filter with the table, keeping any rules on it —
                // rebuilding it from the range would silently clear them.
                if let Some(filter) = table.auto_filter.as_mut() {
                    filter.range = table.range;
                }
                return;
            }
            if within_rows && col == table.range.end.col + 1 {
                table.range.end.col = col;
                // A new column needs a name, or a structured reference to it
                // has nothing to resolve against.
                let next = table.columns.len() + 1;
                let mut name = format!("Column{next}");
                let mut k = next;
                while table
                    .columns
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(&name))
                {
                    k += 1;
                    name = format!("Column{k}");
                }
                table.columns.push(casual_calc_model::TableColumn {
                    id: table.columns.len() as u32 + 1,
                    name,
                    totals_row_function: None,
                    totals_row_label: None,
                    calculated_column_formula: None,
                    totals_row_formula: None,
                });
                // Widen the filter with the table, keeping any rules on it —
                // rebuilding it from the range would silently clear them.
                if let Some(filter) = table.auto_filter.as_mut() {
                    filter.range = table.range;
                }
                return;
            }
        }
    })
}

/// One table as JSON, with its style resolved to concrete colours.
///
/// Shared by `session_table_at` and `session_tables`: the two used to format
/// this separately, which is how `showRowStripes` went out as a bare `1` from
/// one of them while the host compared it to the string `"1"` — banding never
/// painted on any table, and nothing pointed at why.
pub(crate) fn table_json(workbook: &Workbook, t: &Table) -> String {
    let flag = |key: &str| {
        matches!(
            t.style.get(key).map(String::as_str),
            Some("1") | Some("true")
        )
    };
    let style = t.style.get("name").map(String::as_str).unwrap_or_default();
    let c = table_style_colors(workbook, style);
    // The column names as the model holds them, which is what a structured
    // reference resolves against — not the header cells' display text, which
    // can differ once a header is edited.
    let cols: Vec<String> = t.columns.iter().map(|c| json_string(&c.name)).collect();
    format!(
        "{{\"name\":{},\"style\":{},\"r0\":{},\"c0\":{},\"r1\":{},\"c1\":{},\
         \"headers\":{},\"totals\":{},\"stripes\":{},\"colStripes\":{},\
         \"firstCol\":{},\"lastCol\":{},\
         \"headerFill\":{},\"headerText\":{},\"bodyFill\":{},\"bodyText\":{},\
         \"bandFill\":{},\"border\":{},\"cols\":[{}]}}",
        json_string(&t.name),
        json_string(style),
        t.range.start.row,
        t.range.start.col,
        t.range.end.row,
        t.range.end.col,
        t.header_row_count,
        t.totals_row_count,
        flag("showRowStripes"),
        flag("showColumnStripes"),
        flag("showFirstColumn"),
        flag("showLastColumn"),
        json_string(&c.header_fill),
        json_string(&c.header_text),
        json_string(&c.body_fill),
        json_string(&c.body_text),
        json_string(&c.band_fill),
        json_string(&c.border),
        cols.join(","),
    )
}

/// The table covering a cell as JSON, or `null` — drives the UI's state.
#[wasm_bindgen]
pub fn session_table_at(sheet: usize, row: u32, col: u32) -> String {
    with_session(|s| {
        let Some(t) = s.workbook().sheets.get(sheet).and_then(|sh| {
            sh.tables.iter().find(|t| {
                row >= t.range.start.row
                    && row <= t.range.end.row
                    && col >= t.range.start.col
                    && col <= t.range.end.col
            })
        }) else {
            return "null".to_owned();
        };
        table_json(s.workbook(), t)
    })
    .unwrap_or_else(|| "null".to_owned())
}

/// Every table on a sheet, for painting bands and header buttons in one pass.
#[wasm_bindgen]
pub fn session_tables(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<String> = sh
            .tables
            .iter()
            .map(|t| table_json(s.workbook(), t))
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

// ---------------------------------------------------------------------------
// Charts.
//
// A chart read from a file writes back from its own bytes; one made or edited
// here is written from the model. Editing an imported one moves it into the
// second regime and drops the part, because a retained part that no longer
// describes the chart on screen is worse than no part — the same rule a pivot
// table follows.
// ---------------------------------------------------------------------------

/// Every chart's *definition* on a sheet, as JSON.
#[wasm_bindgen]
pub fn session_chart_defs(sheet: usize) -> String {
    with_session(|s| {
        let Some(sh) = s.workbook().sheets.get(sheet) else {
            return "[]".to_owned();
        };
        let items: Vec<ChartWire> = sh
            .charts
            .iter()
            .enumerate()
            .map(|(i, c)| chart_to_wire(c, i))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_owned())
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// A range as `A1:B2`, for display.
pub(crate) fn range_a1(range: &CellRange) -> String {
    let cell = |c: CellRef| {
        format!(
            "{}{}",
            casual_calc_formula::column_to_letters(c.col),
            c.row + 1
        )
    };
    if range.start == range.end {
        cell(range.start)
    } else {
        format!("{}:{}", cell(range.start), cell(range.end))
    }
}

/// A one-line human description of a rule, for the Manage Rules list.
pub(crate) fn describe_cf_rule(rule: &CfRule) -> String {
    match rule {
        CfRule::GreaterThan(x) => format!("greater than {x}"),
        CfRule::LessThan(x) => format!("less than {x}"),
        CfRule::EqualTo(x) => format!("equal to {x}"),
        CfRule::Between(a, b) => format!("between {a} and {b}"),
        CfRule::TextContains(t) => format!("text contains \"{t}\""),
        CfRule::ColorScale(c) => format!("colour scale ({} stops)", c.len()),
        CfRule::DataBar(_) => "data bar".to_owned(),
        CfRule::Top10 {
            rank,
            bottom,
            percent,
        } => format!(
            "{} {rank}{}",
            if *bottom { "bottom" } else { "top" },
            if *percent { "%" } else { "" }
        ),
        CfRule::AboveAverage { below, equal } => format!(
            "{} average{}",
            if *below { "below" } else { "above" },
            if *equal { " (or equal)" } else { "" }
        ),
        CfRule::DuplicateValues { unique } => if *unique {
            "appears only once"
        } else {
            "duplicated"
        }
        .to_owned(),
    }
}

/// Delete the rule at document index `index`.
#[wasm_bindgen]
pub fn session_delete_cf_rule(sheet: usize, index: usize) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if index < data.conditional_formats.len() {
            data.conditional_formats.remove(index);
        }
    })
}

/// Move the rule at `index` earlier (`up`) or later in evaluation order.
///
/// Rewrites every rule's priority to a dense 1..n afterwards, so the order is
/// unambiguous rather than depending on ties broken by document position.
#[wasm_bindgen]
pub fn session_reorder_cf_rule(sheet: usize, index: usize, up: bool) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        let n = data.conditional_formats.len();
        if index >= n {
            return;
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| {
            let p = data.conditional_formats[i].priority;
            (if p == 0 { u32::MAX } else { p }, i)
        });
        let Some(pos) = order.iter().position(|&i| i == index) else {
            return;
        };
        let swap_with = if up {
            if pos == 0 {
                return;
            }
            pos - 1
        } else {
            if pos + 1 >= n {
                return;
            }
            pos + 1
        };
        order.swap(pos, swap_with);
        for (rank, &i) in order.iter().enumerate() {
            data.conditional_formats[i].priority = rank as u32 + 1;
        }
    })
}

/// Turn `stopIfTrue` on or off for the rule at `index`.
#[wasm_bindgen]
pub fn session_set_cf_stop(sheet: usize, index: usize, stop: bool) -> Result<(), JsError> {
    edit_sheet_metadata(sheet, move |_, data| {
        if let Some(cf) = data.conditional_formats.get_mut(index) {
            cf.stop_if_true = stop;
        }
    })
}

/// The cells a formula reads (`deps=false`) or the formulas that read this cell
/// (`deps=true`), as JSON `[{s,r0,c0,r1,c1}]` — blocks, since a range precedent
/// is one arrow, not one per cell.
///
/// Precedents come from the same walk the recalculator uses for its dirty set,
/// so a traced arrow can never point somewhere recalculation would not follow.
#[wasm_bindgen]
pub fn session_trace(sheet: usize, row: u32, col: u32, deps: bool) -> String {
    with_session(|s| {
        let wb = s.workbook();
        let at = CellRef::new(row, col);
        let blocks: Vec<(usize, u32, u32, u32, u32)> = if deps {
            casual_calc_eval::dependents_of(wb, sheet, at)
                .into_iter()
                .map(|(si, r, c)| (si, r, c, r, c))
                .collect()
        } else {
            casual_calc_eval::precedents_of(wb, sheet, at)
        };
        let items: Vec<String> = blocks
            .iter()
            .map(|(si, r0, c0, r1, c1)| {
                format!("{{\"s\":{si},\"r0\":{r0},\"c0\":{c0},\"r1\":{r1},\"c1\":{c1}}}")
            })
            .collect();
        format!("[{}]", items.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Every filter region on the sheet, as JSON
/// `{hidden, regions:[{r0,c0,c1,cols:[absCol,…]}, …]}` — the sheet's own filter
/// first, then each table's.
///
/// `hidden` is how many rows the sheet's filters hide between them. It belongs
/// here rather than on `session_filter_info`, which reports nothing at all when
/// the sheet has no filter of its own: a table's filter would then be reported
/// as hiding nothing, and the status line said "filter cleared" on the edit
/// that had just hidden two rows.
///
/// The host draws a button on every header cell in each region and a "filtered"
/// variant on the columns listed. It needs all of them together: a table's
/// buttons are indistinguishable from the sheet's on screen, and drawing a
/// table's from table geometry alone left them unable to say which of its
/// columns carried a rule.
#[wasm_bindgen]
pub fn session_filter_regions(sheet: usize) -> String {
    with_session(|s| {
        let sh = s.workbook().sheets.get(sheet)?;
        let regions: Vec<String> = sheet_filters(sh)
            .map(|(_, f)| {
                let cols: Vec<String> = f
                    .rules
                    .keys()
                    .map(|off| (f.range.start.col.saturating_add(*off)).to_string())
                    .collect();
                format!(
                    "{{\"r0\":{},\"c0\":{},\"c1\":{},\"cols\":[{}]}}",
                    f.range.start.row,
                    f.range.start.col,
                    f.range.end.col,
                    cols.join(",")
                )
            })
            .collect();
        Some(format!(
            "{{\"hidden\":{},\"regions\":[{}]}}",
            sh.filter_hidden.len(),
            regions.join(",")
        ))
    })
    .flatten()
    .unwrap_or_else(|| "{\"hidden\":0,\"regions\":[]}".to_owned())
}

/// Turn an autofilter on over `r0..=r1 × c0..=c1`, treating the first row as
/// the header. Replaces any existing filter, dropping its rules.
#[wasm_bindgen]
pub fn session_set_filter_range(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
) -> Result<(), JsError> {
    let range = CellRange::new(
        CellRef::new(r0.min(r1), c0.min(c1)),
        CellRef::new(r1.max(r0), c1.max(c0)),
    );
    commit_filter(sheet, FilterSite::Sheet, Some(AutoFilter::new(range)))
}

/// Turn the autofilter off, releasing every row it hid.
#[wasm_bindgen]
pub fn session_clear_filter(sheet: usize) -> Result<(), JsError> {
    commit_filter(sheet, FilterSite::Sheet, None)
}

/// Drop every column rule but keep the filter (and its buttons) in place.
#[wasm_bindgen]
pub fn session_clear_filter_rules(sheet: usize) -> Result<(), JsError> {
    let Some(mut f) = sheet_filter(sheet) else {
        return Ok(());
    };
    f.rules.clear();
    commit_filter(sheet, FilterSite::Sheet, Some(f))
}

/// The distinct values to offer in column `col`'s checklist, as JSON
/// `{"values":[{"v":…,"c":0|1}],"truncated":0|1,"custom":0|1}`.
///
/// `c` is whether the value is currently checked. The list reflects the rows
/// left by the *other* columns' rules, which is what makes chained filtering
/// behave: filtering Region to "West" leaves only West's cities on offer.
/// `custom` flags that this column carries a condition rather than a checklist,
/// so the host can say so instead of showing every box ticked.
/// Which rows a value-set would hide on `col` — **without applying anything**
/// (`COL-32`).
///
/// The personal half of filtering needs the same answer the shared path gets,
/// and must not reach it the same way: `session_set_filter_values` writes
/// `filter_hidden` onto the sheet, which is document state and goes to every
/// participant. This computes the rows and hands them back, leaving the
/// document untouched, so the caller can put them in a personal view instead.
///
/// Deliberately reuses `filter_at_col` and `filter_operands`, the same helpers
/// the shared path uses. Two implementations of "which rows does this value-set
/// hide" would drift, and the drift would show up as a personal filter hiding
/// different rows from the shared one for the same tick-boxes.
///
/// `values` is a JSON array of kept display strings; empty keeps everything.
/// Returns a JSON array of zero-based row indices.
#[wasm_bindgen]
pub fn session_rows_hidden_by_values(sheet: usize, col: u32, values: &str) -> String {
    let kept: Vec<String> = serde_json::from_str(values).unwrap_or_default();
    let out = with_session(|s| {
        let wb = s.workbook();
        let sh = wb.sheets.get(sheet)?;
        let (_, filter) = filter_at_col(sh, col)?;
        // Everything kept means no rule at all, which hides nothing. Matches
        // the shared path, where an all-ticked apply clears the rule rather
        // than storing one that excludes nobody.
        if kept.is_empty() {
            return Some(String::from("[]"));
        }
        let hidden: Vec<String> = (filter.body_start()..=filter.range.end.row)
            .filter(|&row| {
                let value = filter_operands(wb, sh, row, col).0;
                // Case-insensitive, as the shared path's checklist is.
                !kept.iter().any(|k| k.eq_ignore_ascii_case(&value))
            })
            .map(|row| row.to_string())
            .collect();
        Some(format!("[{}]", hidden.join(",")))
    })
    .flatten();
    out.unwrap_or_else(|| "[]".to_owned())
}

#[wasm_bindgen]
pub fn session_filter_values(sheet: usize, col: u32) -> String {
    let empty = "{\"values\":[],\"truncated\":0,\"custom\":0}".to_owned();
    let out = with_session(|s| {
        let wb = s.workbook();
        let sh = wb.sheets.get(sheet)?;
        let (_, filter) = filter_at_col(sh, col)?;
        let off = col - filter.range.start.col;
        let checked: Option<&Vec<String>> = match filter.rules.get(&off) {
            Some(FilterRule::Values(v)) => Some(v),
            _ => None,
        };
        let custom = matches!(filter.rules.get(&off), Some(FilterRule::Custom { .. }));

        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut truncated = false;
        for row in filter.body_start()..=filter.range.end.row {
            if !row_passes(wb, sh, filter, row, Some(off)) {
                continue;
            }
            if seen.len() >= MAX_FILTER_VALUES {
                truncated = true;
                break;
            }
            seen.insert(filter_operands(wb, sh, row, col).0);
        }
        let items: Vec<String> = seen
            .iter()
            .map(|v| {
                // With no checklist on this column every value is on; with one,
                // only the listed values are.
                let on = checked.is_none_or(|c| c.iter().any(|x| x.eq_ignore_ascii_case(v)));
                format!("{{\"v\":{},\"c\":{}}}", json_string(v), u8::from(on))
            })
            .collect();
        Some(format!(
            "{{\"values\":[{}],\"truncated\":{},\"custom\":{}}}",
            items.join(","),
            u8::from(truncated),
            u8::from(custom)
        ))
    })
    .flatten();
    out.unwrap_or(empty)
}

/// Set column `col` to a checklist of `values`. An
/// empty array clears the column's rule rather than hiding every row — a
/// checklist that selects nothing is a user mistake, not an instruction to
/// blank the sheet.
#[wasm_bindgen]
pub fn session_set_filter_values(
    sheet: usize,
    col: u32,
    values: Vec<String>,
) -> Result<(), JsError> {
    let Some((site, mut f)) = filter_for_col(sheet, col) else {
        return Ok(());
    };
    let off = col - f.range.start.col;
    if values.is_empty() {
        f.rules.remove(&off);
    } else {
        f.rules.insert(off, FilterRule::Values(values));
    }
    commit_filter(sheet, site, Some(f))
}

/// Set column `col` to a condition: `op`/`val` and an optional second
/// `op2`/`val2` joined by AND (`and`) or OR.
///
/// `op` names are the OOXML ones (`equal`, `notEqual`, `greaterThan`,
/// `greaterThanOrEqual`, `lessThan`, `lessThanOrEqual`); "contains",
/// "begins with" and "ends with" are `equal` with the host supplying the
/// wildcards, exactly as Excel stores them. An empty `op` clears the column.
#[wasm_bindgen]
pub fn session_set_filter_custom(
    sheet: usize,
    col: u32,
    op: &str,
    val: &str,
    op2: &str,
    val2: &str,
    and: bool,
) -> Result<(), JsError> {
    let Some((site, mut f)) = filter_for_col(sheet, col) else {
        return Ok(());
    };
    let off = col - f.range.start.col;
    if op.is_empty() {
        f.rules.remove(&off);
    } else {
        f.rules.insert(
            off,
            FilterRule::Custom {
                first: CustomFilter {
                    op: FilterOp::from_ooxml(op),
                    value: val.to_owned(),
                },
                second: (!op2.is_empty()).then(|| CustomFilter {
                    op: FilterOp::from_ooxml(op2),
                    value: val2.to_owned(),
                }),
                and,
            },
        );
    }
    commit_filter(sheet, site, Some(f))
}

/// Re-evaluate every sheet's autofilter against the current data.
///
/// Called after a load, where the rows arrive marked `hidden="1"` with no way
/// to tell which of them the filter hid — OOXML records no distinction. Any row
/// this filter would hide is moved out of the hand-hidden set and into the
/// filter's, so clearing the filter later releases exactly those rows. A row
/// hidden by hand that the filter *also* excludes is reattributed to the
/// filter; Excel cannot tell those apart either.
pub(crate) fn reapply_filters_after_load(session: &mut WorkbookSession) {
    let sheet_count = session.workbook().sheets.len();
    for i in 0..sheet_count {
        let wb = session.workbook();
        let Some(sh) = wb.sheets.get(i) else { continue };
        if sh.auto_filter.as_ref().is_none_or(|f| !f.is_active()) {
            continue;
        }
        let hidden = recompute_filter_hidden(wb, sh);
        // Mutate the loaded document in place: this is reconciling what was
        // read, not an edit, so it must not land on the undo stack or dirty the
        // document.
        if let Some(sh) = session.workbook_mut().sheets.get_mut(i) {
            for row in &hidden {
                sh.hidden_rows.remove(row);
            }
            sh.filter_hidden = hidden;
        }
    }
}

/// Set (or clear, with empty hex) the font color across a range (one undo step).
///
/// `theme_slot` is the `theme="N"` index the colour was picked from, or `-1` for
/// a colour with no theme behind it. Passing the slot is what lets the cell move
/// when the workbook is re-themed; a colour picked off the theme row but stored
/// as bare `RRGGBB` stays put forever.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn session_set_font_color(
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    hex: &str,
    theme_slot: i32,
    theme_tint: f64,
) -> Result<(), JsError> {
    let color = (!hex.is_empty()).then(|| hex.to_owned());
    let theme = theme_link(theme_slot, theme_tint);
    apply_style_range(sheet, r0, c0, r1, c1, move |st| {
        st.set_font_color(color.clone(), theme)
    })
}
