//! Structured data over the grid: validation, conditional formats,
//! autofilter and tables.
//!
//! Split out of the single `lib.rs` under `MNT-004`.

use super::*;

/// How many cells a range-backed list will scan, and how many values it keeps.
///
/// A source range is often written `$B:$B` or `$B$1:$B$10000` — the author does
/// not know how long the list will grow. Reading a whole column literally is a
/// million lookups on every frame the grid asks whether to draw a chevron, so
/// the scan is bounded and the answer is the first values found. Excel's own
/// in-cell list stops at 32,767 entries; nobody picks from a thousand.
const LIST_SCAN_BUDGET: usize = 20_000;
const LIST_MAX_VALUES: usize = 1_000;

/// The values a list rule offers, resolved.
///
/// Excel's dropdowns are usually backed by a **range** — `$B$1:$B$20`, kept out
/// of the way on another sheet and maintained on its own — rather than by an
/// inline CSV. The importer preserves that reference in `formula1` and leaves
/// `values` empty, and its own comment said so: *"the rule survives even though
/// the editor cannot offer the dropdown yet."* Both the chevron and the
/// enforcement gated on `!values.is_empty()`, so for the commonest kind of real
/// dropdown there was no list and no rule — the user opened their workbook, the
/// dropdowns were gone, and nothing said why.
///
/// Resolved on read rather than materialised at import, because the list is
/// live: adding a row to the source range adds an option, exactly as in Excel.
/// A rule whose source cannot be parsed returns nothing, which is the same
/// answer as before this existed — never a wrong list.
fn list_values(wb: &Workbook, sheet: usize, v: &DataValidation) -> Vec<String> {
    if !v.values.is_empty() {
        return v.values.clone();
    }
    let source = v.formula1.trim().trim_start_matches('=').trim();
    if source.is_empty() {
        return Vec::new();
    }
    // `Sheet2!$A$1:$A$9`, and the quoted form a sheet with a space in its name
    // gets. The list may live anywhere, which is the point of using a range.
    let (target, area) = match source.rsplit_once('!') {
        Some((name, rest)) => {
            let name = name.trim().trim_matches('\'').replace("''", "'");
            match wb.sheets.iter().position(|sh| sh.name == name) {
                Some(i) => (i, rest),
                None => return Vec::new(),
            }
        }
        None => (sheet, source),
    };
    let (from, to) = match area.split_once(':') {
        Some((a, b)) => (a, b),
        None => (area, area),
    };
    let (Some(a), Some(b)) = (
        casual_calc_formula::parse_a1(from),
        casual_calc_formula::parse_a1(to),
    ) else {
        return Vec::new();
    };
    let Some(sh) = wb.sheets.get(target) else {
        return Vec::new();
    };
    let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
    let (c0, c1) = (a.col.min(b.col), a.col.max(b.col));

    let mut out = Vec::new();
    let mut scanned = 0usize;
    'rows: for r in r0..=r1 {
        for c in c0..=c1 {
            scanned += 1;
            if scanned > LIST_SCAN_BUDGET || out.len() >= LIST_MAX_VALUES {
                break 'rows;
            }
            if let Some(cell) = sh.cells.get(CellRef::new(r, c)) {
                let text = display_text(wb, cell);
                // Blanks in the source are not options — Excel skips them, and
                // a dropdown with empty rows in it is unusable.
                if !text.trim().is_empty() {
                    out.push(text);
                }
            }
        }
    }
    out
}

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
            .filter(|v| v.kind == casual_calc_model::DvKind::List)
            // `showDropDown="1"` *hides* the in-cell list, as the schema
            // defines it. A file that asked for a typed-only list was still
            // getting a chevron.
            .filter(|v| !v.hide_dropdown)
        {
            Some(v) => {
                let values = list_values(s.workbook(), sheet, v);
                // Still `null` when the list resolves to nothing: an empty array
                // is truthy in JavaScript, so the host would draw a chevron that
                // opens onto an empty menu.
                if values.is_empty() {
                    return "null".to_owned();
                }
                let items: Vec<String> = values.iter().map(|x| json_string(x)).collect();
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
        // A range-backed list is decided here, because the model only knows the
        // literal `values` and this rule's are in cells. Confined to the case
        // where `values` is empty, so an inline list keeps exactly the
        // semantics it had — this adds enforcement where there was none rather
        // than changing enforcement that worked.
        if rule.kind == casual_calc_model::DvKind::List && rule.values.is_empty() {
            let resolved = list_values(s.workbook(), sheet, rule);
            if resolved.is_empty() {
                // Nothing to check against — an unreadable source is not a
                // reason to refuse what somebody typed.
                return String::new();
            }
            // Excel matches a list case-insensitively.
            if resolved.iter().any(|v| v.eq_ignore_ascii_case(trimmed)) {
                return String::new();
            }
            if !rule.error_text.is_empty() {
                return rule.error_text.clone();
            }
            let shown: Vec<&str> = resolved.iter().take(6).map(String::as_str).collect();
            let ellipsis = if resolved.len() > shown.len() {
                ", …"
            } else {
                ""
            };
            return format!("must be one of: {}{ellipsis}", shown.join(", "));
        }
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

/// The **whole** rule covering a cell, as JSON, or `""` when the cell has no
/// rule. The panel loads this so reopening Data ▸ Validation shows what is
/// already set instead of an empty dialog.
///
/// `{kind, operator, formula1, formula2, values, allowBlank, style, errorTitle,
/// errorText, promptTitle, promptText, hideDropdown}`.
///
/// **`kind` and `operator` are the OOXML tokens**, the same spelling
/// `session_set_validation` takes, so the panel can hand back what it was given
/// without a translation table in the middle — one of those is one place for the
/// two halves to disagree. `formula1`/`formula2` are the operands verbatim.
///
/// **A list rule's source is `values` *or* `formula1`, never both**, and the
/// dialog needs the distinction: a literal list fills the options box, a range
/// fills the source box. [`session_validation_at`] is not a substitute — it
/// resolves the source down to the options the *cell's* dropdown shows, and
/// deliberately answers `null` for a rule with `hide_dropdown` set or one whose
/// range resolves to nothing. Both of those are rules the user can still open
/// and must still be able to see.
///
/// The empty string, not an object of defaults, when there is no rule: a `none`
/// rule the author never set is indistinguishable in the dialog from one they
/// did.
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
        let values: Vec<String> = r.values.iter().map(|v| json_string(v)).collect();
        Some(format!(
            "{{\"kind\":{},\"operator\":{},\"formula1\":{},\"formula2\":{},\
             \"values\":[{}],\"allowBlank\":{},\
             \"style\":{},\"errorTitle\":{},\"errorText\":{},\
             \"promptTitle\":{},\"promptText\":{},\"hideDropdown\":{}}}",
            json_string(r.kind.ooxml()),
            json_string(r.operator.ooxml()),
            json_string(&r.formula1),
            json_string(&r.formula2),
            values.join(","),
            r.allow_blank,
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

/// The rule a `kind` token and its operands name, or why it is not one.
///
/// Split out of [`session_add_cf`] because the refusals are the interesting
/// part and `JsError` cannot be constructed off a WebAssembly target — a test
/// that asks whether a bad formula is refused would abort rather than answer.
/// The error is a plain `String` here and becomes a `JsError` one line up.
fn cf_rule_from_kind(kind: &str, a: f64, b: f64, text: &str) -> Result<CfRule, String> {
    Ok(match kind {
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
                return Err("a colour scale needs at least two colours".to_owned());
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
        // The custom-formula rule, and the only way to highlight a whole row:
        // `$D2>100` over `A2:H10`. The formula arrives through `text` and is
        // **anchored to the top-left of the range** — the top-left the caller
        // sorts `r0`/`r1` into — which is both what OOXML means by it and what
        // an Excel user types. A leading `=` is what the dialog shows, so it is
        // accepted and dropped; the model holds the body.
        "formula" => {
            let body = text.trim().strip_prefix('=').unwrap_or(text.trim()).trim();
            if body.is_empty() {
                return Err("a formula rule needs a formula".to_owned());
            }
            // Refused here rather than stored: a formula that does not parse
            // can never match, so a rule holding one is a highlight that will
            // never appear and never say why.
            casual_calc_formula::parse(body)
                .map_err(|e| format!("that formula does not parse: {e}"))?;
            CfRule::Expression(body.to_owned())
        }
        _ => return Err("unknown conditional-format rule".to_owned()),
    })
}

/// Add a highlight-cells conditional-format rule over a range. `kind` is one of
/// `gt`/`lt`/`eq`/`between`/`contains`/`formula`; `a`/`b` are numeric operands
/// (b only for `between`), `text` the substring for `contains` or the formula
/// body for `formula`, `fill` the `RRGGBB` color.
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
    let rule = cf_rule_from_kind(kind, a, b, text).map_err(|e| JsError::new(&e))?;
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
        CfRule::GreaterThanOrEqual(x) => format!("greater than or equal to {x}"),
        CfRule::LessThanOrEqual(x) => format!("less than or equal to {x}"),
        CfRule::NotEqualTo(x) => format!("not equal to {x}"),
        CfRule::NotBetween(a, b) => format!("not between {a} and {b}"),
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
        // Shown with the `=` an author writes, even though the model stores the
        // body: the rules manager is where somebody checks what a rule says,
        // and a formula without its `=` reads as a label.
        CfRule::Expression(f) => format!("formula ={f}"),
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

/// Which kind a checklist value belongs to, and therefore which block of the
/// list it sits in: `0` numeric, `1` text, `2` blank.
///
/// A date is a number wearing a format, so it lands in the numeric block and
/// orders by its serial — which is chronological order, for free. Anything the
/// model does not hold as a number is text, including a number that was *typed*
/// as text: the two are indistinguishable in the checklist (both are display
/// strings) but not in the sheet, and putting the text "10" beside the number
/// 10 would claim they filter the same, which they do not.
fn filter_value_kind(text: &str, num: Option<f64>) -> u8 {
    if text.is_empty() {
        2
    } else if num.is_some() {
        0
    } else {
        1
    }
}

/// A **total** order over checklist values: numbers first (ascending), then
/// text (case-insensitive A→Z), then blanks — Excel's order, and the one the
/// menu is read expecting.
///
/// Totality is the point, not a nicety. The list is rebuilt every time the
/// dropdown opens, so any pair the comparison leaves genuinely equal is free to
/// swap between openings and the menu flickers. Hence the two tie-breaks:
/// `total_cmp` rather than `partial_cmp` (a NaN must still have a fixed place
/// rather than making the sort's job undefined), a case-insensitive comparison
/// so `apple` and `Banana` interleave the way a reader expects, and finally the
/// raw bytes so that `apple` and `Apple` — equal case-insensitively, and two
/// distinct entries in the list — still have a fixed order between them.
fn filter_value_order(a: &(String, Option<f64>), b: &(String, Option<f64>)) -> Ordering {
    let (at, an) = (a.0.as_str(), a.1);
    let (bt, bn) = (b.0.as_str(), b.1);
    filter_value_kind(at, an)
        .cmp(&filter_value_kind(bt, bn))
        .then_with(|| match (an, bn) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => Ordering::Equal,
        })
        .then_with(|| {
            at.chars()
                .flat_map(char::to_lowercase)
                .cmp(bt.chars().flat_map(char::to_lowercase))
        })
        .then_with(|| at.cmp(bt))
}

/// The distinct values to offer in column `col`'s checklist, as JSON
/// `{"values":[{"v":…,"c":0|1}],"truncated":0|1,"custom":0|1}`.
///
/// `c` is whether the value is currently checked. The list reflects the rows
/// left by the *other* columns' rules, which is what makes chained filtering
/// behave: filtering Region to "West" leaves only West's cities on offer.
/// `custom` flags that this column carries a condition rather than a checklist,
/// so the host can say so instead of showing every box ticked.
///
/// The order is `filter_value_order`'s, and it is part of the contract: the
/// host draws the array as given. It used to be the order of a
/// `BTreeSet<String>` of display text — byte order — which is alphabetical
/// order applied to digits, so a column holding 9, 10, 100 and 2 listed as
/// `10, 100, 2, 9` and read as broken.
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

        // Dedup still keys on the display text — that is what a tick-box
        // stands for, and what `session_set_filter_values` is given back — but
        // each value now carries the number behind it, because ordering by the
        // text alone is what put 100 before 2. The first row to show a given
        // text supplies its number: two rows can display the same text from
        // different values (1.0000001 and 1.0000002 under a 2-decimal format),
        // and "first one wins" is a rule that does not depend on the scan
        // finding them in a different order next time.
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut values: Vec<(String, Option<f64>)> = Vec::new();
        let mut truncated = false;
        for row in filter.body_start()..=filter.range.end.row {
            if !row_passes(wb, sh, filter, row, Some(off)) {
                continue;
            }
            if seen.len() >= MAX_FILTER_VALUES {
                truncated = true;
                break;
            }
            let (text, num) = filter_operands(wb, sh, row, col);
            if seen.insert(text.clone()) {
                values.push((text, num));
            }
        }
        values.sort_by(filter_value_order);
        let items: Vec<String> = values
            .iter()
            .map(|(v, _)| {
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

// ---------------------------------------------------------------------------
// Column statistics.
//
// Google Sheets' *Data ▸ Column stats*: what a column actually holds, for
// somebody who has just been sent it. The status bar's `session_range_stats`
// answers the same question at a glance (Sum/Avg/Min/Max/Count) and this must
// agree with it — a panel that disagrees with the bar three centimetres below
// it is worse than no panel — so the numeric aggregate keeps the bar's rule
// that every `Number` cell is numeric, dates included, and adds the shape the
// bar has no room for: medians, deviations, frequencies, and which kinds of
// value are mixed together in the column.
//
// Three rules decide almost every answer here:
//
// - **A blank is not a zero.** Empty cells are counted on their own line and
//   enter no aggregate; averaging them as zeroes hides the gap that caused the
//   number to look wrong.
// - **A number stored as text is text.** It is the commonest defect in a real
//   column and the reason to open the panel at all, so it is counted as text
//   *and* named (`types.numberAsText`) rather than quietly coerced.
// - **An error is a value.** `#DIV/0!` is counted, broken down by token, and is
//   never a number.
// ---------------------------------------------------------------------------

/// What one column-stats pass will look at, and how much of it comes back.
#[derive(Clone, Copy)]
pub(crate) struct StatsLimits {
    /// Stored (non-blank) cells examined before the answer is marked partial.
    pub scan: usize,
    /// Distinct values tracked for `unique` and the frequency table.
    pub distinct: usize,
    /// Bytes of value text held in that table.
    pub key_bytes: usize,
    /// Frequency rows returned.
    pub top: usize,
}

impl Default for StatsLimits {
    fn default() -> Self {
        Self {
            // Two million: more than a whole column (1,048,576), so the case the
            // panel is opened for is never truncated, and still a hard stop for
            // a many-column selection over a full sheet — where the rectangle is
            // 17 billion cells and no answer is worth that wait.
            scan: 2_000_000,
            // A column of 100,000 unique invoice numbers is a real column. It is
            // tracked (the count is exact) but the table it feeds is not
            // returned; past this the answer becomes a lower bound and says so
            // through `uniqueExact`.
            distinct: 100_000,
            // …unless the values are long. 100,000 × 32 KB of text would be 3 GB
            // in a tab, so the map is capped by bytes as well as by entries.
            key_bytes: 4 << 20,
            // Ten rows, the length of Sheets' own list: a frequency table is
            // read to spot the modal values and the odd one out, and nobody
            // reads the eleventh. Everything else is summed into `frequencyOther`
            // rather than dropped.
            top: 10,
        }
    }
}

/// The kinds a stats pass distinguishes. Ordered, because the frequency table's
/// tie-break has to be total or the panel reshuffles between identical runs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ValueKind {
    Number,
    Date,
    Text,
    Bool,
    Error,
}

impl ValueKind {
    fn tag(self) -> &'static str {
        match self {
            ValueKind::Number => "number",
            ValueKind::Date => "date",
            ValueKind::Text => "text",
            ValueKind::Bool => "boolean",
            ValueKind::Error => "error",
        }
    }
}

/// A float as JSON, or `null` where JSON has no spelling for it.
///
/// `format!("{}", f64::NAN)` is `NaN`, which is not JSON: emitting it would
/// throw inside the host's `JSON.parse` and take the whole panel out over one
/// cell. Non-finite values reach here from imported files.
fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

/// How many of each kind of value the range holds.
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TypeCounts {
    pub number: u64,
    pub date: u64,
    pub text: u64,
    pub number_as_text: u64,
    pub boolean: u64,
    pub error: u64,
    pub formula: u64,
}

/// The numeric aggregate over the range.
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NumericStats {
    pub count: u64,
    pub sum: Option<f64>,
    pub avg: Option<f64>,
    pub median: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub stdev: Option<f64>,
    pub stdevp: Option<f64>,
}

/// One row of the frequency table.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FreqEntry {
    pub value: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub count: u64,
}

/// Everything the frequency table did not list.
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FreqOther {
    pub values: u64,
    pub count: u64,
}

/// The whole answer.
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ColumnStats {
    pub rows: u64,
    pub cols: u64,
    pub cells: u64,
    pub count: u64,
    pub empty: u64,
    pub unique: u64,
    pub unique_exact: bool,
    pub truncated: bool,
    pub types: TypeCounts,
    pub errors: std::collections::BTreeMap<String, u64>,
    pub numeric: NumericStats,
    pub frequency: Vec<FreqEntry>,
    pub frequency_other: FreqOther,
}

/// The selection as `(r0, c0, r1, c1)`, normalised and inside the grid.
///
/// The host may hand over a backwards drag, and a row past the last one would
/// make `cells` — and so the empty count — a number about an address space that
/// does not exist.
fn clamp_rect(r0: u32, c0: u32, r1: u32, c1: u32) -> (u32, u32, u32, u32) {
    use casual_calc_model::{GRID_MAX_COL, GRID_MAX_ROW};
    (
        r0.min(r1).min(GRID_MAX_ROW),
        c0.min(c1).min(GRID_MAX_COL),
        r0.max(r1).min(GRID_MAX_ROW),
        c0.max(c1).min(GRID_MAX_COL),
    )
}

impl ColumnStats {
    /// The answer for a range with nothing in it: every cell empty, and every
    /// bound still honest. Also what a caller with no session or no such sheet
    /// gets, so the host never has to branch on a differently shaped reply.
    fn over(r0: u32, c0: u32, r1: u32, c1: u32) -> Self {
        let (lo_row, lo_col, hi_row, hi_col) = clamp_rect(r0, c0, r1, c1);
        let rows = u64::from(hi_row - lo_row) + 1;
        let cols = u64::from(hi_col - lo_col) + 1;
        Self {
            rows,
            cols,
            cells: rows * cols,
            empty: rows * cols,
            unique_exact: true,
            ..Self::default()
        }
    }
}

/// Summarise `r0..=r1 × c0..=c1` on `sheet`.
///
/// Walks the **stored** cells in the row band, not the rectangle: a panel is
/// opened on `A:A`, which addresses 1,048,576 cells and usually holds a few
/// hundred, and the empty count is then arithmetic (`cells - count`) rather
/// than a million lookups that find nothing.
///
/// `limits` is a parameter rather than a constant so the bounds are testable
/// without a two-million-cell fixture.
#[allow(clippy::too_many_arguments)]
pub(crate) fn column_stats(
    wb: &Workbook,
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    limits: StatsLimits,
) -> ColumnStats {
    use std::collections::hash_map::Entry;

    let (lo_row, lo_col, hi_row, hi_col) = clamp_rect(r0, c0, r1, c1);
    let mut out = ColumnStats::over(r0, c0, r1, c1);
    let Some(sh) = wb.sheets.get(sheet) else {
        return out;
    };

    // Every numeric value, kept because the median needs them ordered and a
    // two-pass deviation is worth the memory over the naive `E[x²] - mean²`,
    // which loses its significant digits on a column of large near-equal
    // numbers (timestamps, prices). Bounded by `limits.scan`.
    let mut nums: Vec<f64> = Vec::new();
    // (kind, exact value text) → (occurrences, the text as displayed). Keyed on
    // the exact value so `42` and `'42` are two values — which is the whole
    // point — and labelled with the displayed one so a date column's frequency
    // rows read `2024-01-15` rather than `45306`.
    let mut freq: std::collections::HashMap<(ValueKind, String), (u64, String)> =
        std::collections::HashMap::new();
    let mut tracked = 0u64;
    let mut key_bytes = 0usize;
    // Occurrences of values there was no room to track. They are still counted,
    // in `frequencyOther` — dropping them would make the table's totals lie.
    let mut untracked = 0u64;

    // `examined` counts every stored cell the band hands over, including the
    // ones outside the selected columns: skipping a cell is cheap but not free,
    // and the budget has to bound the walk rather than the part of it that
    // happens to be in the selection.
    for (examined, (at, cell)) in sh.cells.row_band(lo_row, hi_row).enumerate() {
        if examined >= limits.scan {
            out.truncated = true;
            break;
        }
        if at.col < lo_col || at.col > hi_col {
            continue;
        }
        // A cell exists in the store for its style or its comment alone, and a
        // formula can evaluate to nothing. None of those is a value: they are
        // empty, and they are counted by the arithmetic below rather than here.
        if cell.value.is_empty() {
            continue;
        }
        out.count += 1;
        if cell.formula.is_some() {
            out.types.formula += 1;
        }

        let (kind, key) = match &cell.value {
            CellValue::Number(n) => {
                out.numeric.count += 1;
                nums.push(*n);
                // A date is a number wearing a format, and the status bar counts
                // it as one. Named separately all the same: "300 dates" and "300
                // numbers" are different columns.
                let is_date = casual_calc_layout::cell_number_format(wb, cell)
                    .is_some_and(casual_calc_io::is_date_format);
                if is_date {
                    out.types.date += 1;
                    (ValueKind::Date, format!("{n}"))
                } else {
                    out.types.number += 1;
                    (ValueKind::Number, format!("{n}"))
                }
            }
            CellValue::Bool(b) => {
                out.types.boolean += 1;
                (
                    ValueKind::Bool,
                    if *b { "TRUE" } else { "FALSE" }.to_owned(),
                )
            }
            CellValue::Error(e) => {
                out.types.error += 1;
                *out.errors.entry(e.to_string()).or_default() += 1;
                (ValueKind::Error, e.to_string())
            }
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                let text = wb.strings.get(*id).unwrap_or_default();
                out.types.text += 1;
                // Counted as text — never coerced — and *named*, because one
                // `'007` in a numeric column is why the SUM is short and the
                // panel exists to say so.
                if text.trim().parse::<f64>().is_ok_and(f64::is_finite) {
                    out.types.number_as_text += 1;
                }
                (ValueKind::Text, text.to_owned())
            }
            CellValue::Empty => continue,
        };

        let cost = key.len();
        match freq.entry((kind, key)) {
            Entry::Occupied(mut seen) => seen.get_mut().0 += 1,
            Entry::Vacant(slot) => {
                if tracked < limits.distinct as u64 && key_bytes + cost <= limits.key_bytes {
                    slot.insert((1, display_text(wb, cell)));
                    tracked += 1;
                    key_bytes += cost;
                } else {
                    untracked += 1;
                    // `unique` is now a floor, and the host must not print it as
                    // a fact.
                    out.unique_exact = false;
                }
            }
        }
    }

    out.empty = out.cells.saturating_sub(out.count);
    out.unique = tracked;

    if !nums.is_empty() {
        // `total_cmp`: a total order, so a NaN that came in from a file cannot
        // make the sort itself nondeterministic.
        nums.sort_unstable_by(f64::total_cmp);
        let n = nums.len();
        let sum: f64 = nums.iter().sum();
        let mean = sum / n as f64;
        let median = if n % 2 == 1 {
            nums[n / 2]
        } else {
            (nums[n / 2 - 1] + nums[n / 2]) / 2.0
        };
        let ss: f64 = nums.iter().map(|v| (v - mean) * (v - mean)).sum();
        out.numeric.sum = finite(sum);
        out.numeric.avg = finite(mean);
        out.numeric.median = finite(median);
        out.numeric.min = finite(nums[0]);
        out.numeric.max = finite(nums[n - 1]);
        // Sample (n-1) *and* population, as Excel's STDEV.S and STDEV.P: with
        // one value the sample deviation is undefined rather than zero, and
        // reporting zero there is a claim about spread that nothing supports.
        out.numeric.stdev = (n > 1)
            .then(|| (ss / (n - 1) as f64).sqrt())
            .and_then(finite);
        out.numeric.stdevp = finite((ss / n as f64).sqrt());
    }

    // Most frequent first; ties by kind then by value, so two runs over the same
    // data return the same rows in the same order. A `HashMap`'s own order is
    // randomised per process, and a panel that reshuffles on every open reads as
    // broken.
    let mut ranked: Vec<(ValueKind, String, u64, String)> = freq
        .into_iter()
        .map(|((kind, key), (count, label))| (kind, key, count, label))
        .collect();
    ranked.sort_unstable_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });
    let listed = ranked.len().min(limits.top);
    out.frequency_other.values = (ranked.len() - listed) as u64;
    out.frequency_other.count = ranked[listed..].iter().map(|e| e.2).sum::<u64>() + untracked;
    out.frequency = ranked
        .into_iter()
        .take(listed)
        .map(|(kind, _, count, label)| FreqEntry {
            value: label,
            kind: kind.tag().to_owned(),
            count,
        })
        .collect();
    out
}

/// Summarise a column (or any range) the way Google Sheets' *Column stats* does:
/// how many rows, how many blanks, how many distinct values, the numeric
/// aggregate (sum/avg/median/min/max/deviation), the most frequent values, and
/// the mix of *kinds* — which is how the one text cell wrecking a SUM becomes
/// visible.
///
/// Returns JSON:
///
/// ```json
/// {
///   "rows": 5, "cols": 1, "cells": 5, "count": 3, "empty": 2,
///   "unique": 3, "uniqueExact": true, "truncated": false,
///   "types": { "number": 2, "date": 0, "text": 1, "numberAsText": 1,
///              "boolean": 0, "error": 0, "formula": 0 },
///   "errors": { "#DIV/0!": 1 },
///   "numeric": { "count": 2, "sum": 30.0, "avg": 15.0, "median": 15.0,
///                "min": 10.0, "max": 20.0, "stdev": 7.07, "stdevp": 5.0 },
///   "frequency": [ { "value": "007", "type": "text", "count": 1 } ],
///   "frequencyOther": { "values": 0, "count": 0 }
/// }
/// ```
///
/// `count` is non-empty cells and `empty` is the rest of the rectangle;
/// `types` partitions `count` (`formula` cuts across it, and `numberAsText` is
/// the subset of `text` that parses as a number). `numeric.count` is
/// `types.number + types.date` — a date is a number wearing a format, which is
/// how `session_range_stats` counts it for the status bar, and the panel must
/// not contradict the bar. Every `numeric` field is `null` when there is
/// nothing to report — including when one non-finite value poisons the
/// aggregate, since JSON has no other way to say so.
///
/// `truncated` and `uniqueExact` say when a bound was hit rather than letting a
/// partial answer pass as a whole one: with `truncated` set, every count
/// describes the prefix that was scanned, and `empty` (which is arithmetic over
/// the rectangle) is then a ceiling rather than a fact.
#[wasm_bindgen]
pub fn session_column_stats(sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) -> String {
    let stats =
        with_session(|s| column_stats(s.workbook(), sheet, r0, c0, r1, c1, StatsLimits::default()))
            .unwrap_or_else(|| ColumnStats::over(r0, c0, r1, c1));
    serde_json::to_string(&stats).unwrap_or_else(|_| "null".to_owned())
}

#[cfg(test)]
mod filter_value_order_tests {
    use super::{
        session_filter_values, session_new, session_set_cell, session_set_filter_range,
        session_set_number_format,
    };

    /// The checklist's values, in the order the host will draw them.
    fn listed(col: u32) -> Vec<String> {
        let payload: serde_json::Value =
            serde_json::from_str(&session_filter_values(0, col)).expect("payload is json");
        payload["values"]
            .as_array()
            .expect("values is an array")
            .iter()
            .map(|v| v["v"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Fill column A with a header and `body`, then filter over it.
    fn column(body: &[&str]) {
        session_new();
        session_set_cell(0, 0, 0, "n").unwrap();
        for (i, v) in body.iter().enumerate() {
            session_set_cell(0, i as u32 + 1, 0, v).unwrap();
        }
        session_set_filter_range(0, 0, 0, body.len() as u32, 0).unwrap();
    }

    /// **A column of numbers lists numerically, not lexicographically.**
    ///
    /// The whole defect in one assertion: byte order over display text is
    /// alphabetical order over digits, so `9, 10, 100, 2` listed as
    /// `10, 100, 2, 9`.
    #[test]
    fn numbers_list_in_numeric_order() {
        column(&["9", "10", "100", "2"]);
        assert_eq!(listed(0), ["2", "9", "10", "100"]);
    }

    /// **Numbers first, then text, then blanks — and text is case-insensitive.**
    ///
    /// The three kinds do not interleave: a reader scanning for a number should
    /// not have to step over "Zebra" to reach 100. Case-insensitivity is the
    /// other half of the same complaint — byte order puts every capital ahead
    /// of every lowercase, so `Zebra` sorted before `apple`.
    #[test]
    fn kinds_do_not_interleave_and_text_ignores_case() {
        column(&["Zebra", "9", "", "apple", "100", "Apple", "2"]);
        assert_eq!(listed(0), ["2", "9", "100", "Apple", "apple", "Zebra", ""]);
    }

    /// **A number typed as text sorts with the text, not with the numbers.**
    ///
    /// `'007` is a part number, not seven. It filters as the string `007`, and
    /// listing it beside the number 7 would claim otherwise.
    #[test]
    fn a_number_held_as_text_is_text() {
        column(&["7", "'007", "2"]);
        assert_eq!(listed(0), ["2", "7", "007"]);
    }

    /// **Dates order chronologically, because they order as their serials.**
    ///
    /// Nothing in the comparison knows what a date is: it is a number wearing a
    /// format, and ordering the number gets the calendar right for free.
    ///
    /// Worn deliberately here. Under the default ISO rendering a byte sort of
    /// the display text is *accidentally* chronological, so a test that used it
    /// would pass against the old code and prove nothing; `dd/mm/yyyy` puts the
    /// day in front, where byte order sorts `01/03/2024` (March) ahead of
    /// `31/12/2023` (December).
    #[test]
    fn dates_order_chronologically() {
        column(&["2024-03-01", "2023-12-31", "2024-01-02"]);
        session_set_number_format(0, 1, 0, 3, 0, "dd/mm/yyyy").unwrap();
        assert_eq!(listed(0), ["31/12/2023", "02/01/2024", "01/03/2024"]);
    }

    /// **The order is a total order, so the menu cannot flicker.**
    ///
    /// The list is rebuilt every time the dropdown opens. Any pair the
    /// comparison leaves equal is free to swap between openings, which shows up
    /// as values jumping about under the cursor — so the same sheet must give
    /// the same list every time it is asked.
    #[test]
    fn the_same_column_lists_the_same_way_every_time() {
        column(&["Apple", "apple", "10", "10.0", "", "APPLE", "2"]);
        let first = listed(0);
        for _ in 0..5 {
            assert_eq!(listed(0), first);
        }
        // Values equal case-insensitively are still distinct entries with a
        // fixed order between them, rather than an arbitrary one.
        assert_eq!(
            first
                .iter()
                .filter(|v| v.eq_ignore_ascii_case("apple"))
                .count(),
            3
        );
    }
}

/// The filter menu's "Sort A→Z", checked at the seam the browser cannot be
/// asked about here: which block gets sorted, and whether the heading survives.
///
/// `filterSortRange` in `webapp/editor.dialogs.js` is arithmetic over three
/// bindings' JSON, and `sortFilterColumn` hands the result to
/// `session_sort_range_multi` with `hasHeader` true. Every one of those calls
/// is reproduced below, in order and with the same arguments, so the range the
/// host computes is exercised rather than asserted from reading.
#[cfg(test)]
mod filter_sort_range_tests {
    use super::{
        session_cells, session_create_table, session_filter_info, session_filter_regions,
        session_hide_rows, session_new, session_set_cell, session_set_filter_range,
        session_set_filter_values, session_sort_range_multi, session_table_at,
        session_table_totals, session_undo, with_session,
    };

    /// One cell's displayed text.
    fn shown(row: u32, col: u32) -> String {
        let cells: Vec<serde_json::Value> =
            serde_json::from_str(&session_cells(0, row, col, row, col)).expect("cells are json");
        cells
            .iter()
            .find(|c| c["r"] == row && c["c"] == col)
            .and_then(|c| c["t"].as_str())
            .unwrap_or_default()
            .to_owned()
    }

    /// `filterSortRange(col)`, transcribed from `webapp/editor.dialogs.js`.
    fn filter_sort_range(col: u32) -> Option<(u32, u32, u32, u32)> {
        let regions: serde_json::Value =
            serde_json::from_str(&session_filter_regions(0)).expect("regions are json");
        let regions = regions["regions"].as_array().expect("an array").clone();
        let num = |v: &serde_json::Value, k: &str| v[k].as_u64().unwrap_or_default() as u32;
        let i = regions
            .iter()
            .position(|r| col >= num(r, "c0") && col <= num(r, "c1"))?;
        let region = &regions[i];
        let info = session_filter_info(0);
        if i == 0 && info != "null" {
            let info: serde_json::Value = serde_json::from_str(&info).expect("info is json");
            return Some((
                num(region, "r0"),
                num(region, "c0"),
                num(&info, "r1"),
                num(region, "c1"),
            ));
        }
        let table = session_table_at(0, num(region, "r0"), col);
        if table == "null" {
            return None;
        }
        let table: serde_json::Value = serde_json::from_str(&table).expect("table is json");
        Some((
            num(region, "r0"),
            num(region, "c0"),
            num(&table, "r1") - num(&table, "totals"),
            num(region, "c1"),
        ))
    }

    /// `sortFilterColumn(col, asc)`: the range, then the sort one row down.
    fn sort_from_filter_menu(col: u32, asc: bool) {
        let (r0, c0, r1, c1) = filter_sort_range(col).expect("the column is under a filter");
        session_sort_range_multi(0, r0 + 1, c0, r1, c1, vec![col], vec![u8::from(asc)]).unwrap();
    }

    /// Two columns of three data rows under a heading.
    fn grid() {
        session_new();
        for (r, (a, b)) in [("n", "tag"), ("3", "c"), ("1", "a"), ("2", "b")]
            .iter()
            .enumerate()
        {
            session_set_cell(0, r as u32, 0, a).unwrap();
            session_set_cell(0, r as u32, 1, b).unwrap();
        }
    }

    /// **The heading stays put, and the whole row travels with its key.**
    ///
    /// `hasHeader` is `true` by construction here rather than
    /// `looksLikeHeader`'s guess. Pass `false` (sort from `r0` instead of
    /// `r0 + 1`) and the text heading sorts after the numbers, which is what
    /// this asserts against.
    #[test]
    fn a_sheet_filter_sorts_its_body_and_keeps_its_header() {
        grid();
        session_set_filter_range(0, 0, 0, 3, 1).unwrap();
        assert_eq!(filter_sort_range(0), Some((0, 0, 3, 1)));
        sort_from_filter_menu(0, true);
        assert_eq!(
            (0..4).map(|r| shown(r, 0)).collect::<Vec<_>>(),
            ["n", "1", "2", "3"]
        );
        // The neighbouring column came along: sorting a key column alone would
        // silently decouple every row from its own data.
        assert_eq!(
            (0..4).map(|r| shown(r, 1)).collect::<Vec<_>>(),
            ["tag", "a", "b", "c"]
        );
        sort_from_filter_menu(0, false);
        assert_eq!(
            (0..4).map(|r| shown(r, 0)).collect::<Vec<_>>(),
            ["n", "3", "2", "1"]
        );
    }

    /// **A table's filter sorts the table, not the block around it.**
    ///
    /// The row below the table is data the user put there; the surrounding
    /// block would swallow it, and sorting would move it into the table.
    #[test]
    fn a_table_filter_sorts_only_the_table() {
        grid();
        session_set_cell(0, 4, 0, "0").unwrap();
        session_set_cell(0, 4, 1, "outside").unwrap();
        session_create_table(0, 0, 0, 3, 1, "T", true).unwrap();
        // No sheet filter, so region 0 is the table's own.
        assert_eq!(session_filter_info(0), "null");
        assert_eq!(filter_sort_range(0), Some((0, 0, 3, 1)));
        sort_from_filter_menu(0, true);
        assert_eq!(
            (0..5).map(|r| shown(r, 0)).collect::<Vec<_>>(),
            ["n", "1", "2", "3", "0"]
        );
        assert_eq!(shown(4, 1), "outside");
    }

    /// **A totals row is not sorted with the data.**
    ///
    /// The table's range grows to cover it, so the end row has to come back off
    /// again — sorting it in would rank the total among the values and leave a
    /// SUBTOTAL sitting in the middle of the column.
    #[test]
    fn a_totals_row_stays_at_the_bottom() {
        grid();
        session_create_table(0, 0, 0, 3, 1, "T", true).unwrap();
        session_table_totals(0, 1, 0, true).unwrap();
        let table: serde_json::Value =
            serde_json::from_str(&session_table_at(0, 0, 0)).expect("table is json");
        assert_eq!(table["r1"], 4, "the totals row grew the table's range");
        assert_eq!(table["totals"], 1);
        // …and comes straight back off, leaving the body at rows 1..=3.
        assert_eq!(filter_sort_range(0), Some((0, 0, 3, 1)));
        let totals_before = shown(4, 1);
        sort_from_filter_menu(0, true);
        assert_eq!(
            (0..4).map(|r| shown(r, 0)).collect::<Vec<_>>(),
            ["n", "1", "2", "3"]
        );
        assert_eq!(shown(4, 1), totals_before, "the totals row did not move");
    }

    /// Which rows the sheet's filters currently hide, read straight off the
    /// document rather than through a count.
    ///
    /// `session_filter_info` reports `hidden` as a *length*, and a length is
    /// exactly what this defect leaves correct — one row hidden before the
    /// sort, one row hidden after, the wrong one.
    fn filter_hidden() -> Vec<u32> {
        with_session(|s| {
            s.workbook().sheets[0]
                .filter_hidden
                .iter()
                .copied()
                .collect()
        })
        .unwrap_or_default()
    }

    /// Whether each body row is hidden, in row order.
    fn hidden_flags() -> Vec<bool> {
        with_session(|s| {
            let sh = &s.workbook().sheets[0];
            (1..=3).map(|r| sh.is_row_hidden(r)).collect()
        })
        .unwrap_or_default()
    }

    /// Three data rows under a heading with a `region` filter keeping `West`,
    /// so the `East` row at index 2 is the one hidden.
    fn filtered_grid() {
        session_new();
        for (r, (region, n)) in [("region", "n"), ("West", "3"), ("East", "1"), ("West", "2")]
            .iter()
            .enumerate()
        {
            session_set_cell(0, r as u32, 0, region).unwrap();
            session_set_cell(0, r as u32, 1, n).unwrap();
        }
        session_set_filter_range(0, 0, 0, 3, 1).unwrap();
        session_set_filter_values(0, 0, vec!["West".to_owned()]).unwrap();
    }

    /// **`DATA-DUP-01`: Remove Duplicates sees only the rows the filter shows.**
    ///
    /// It scanned every row in the band, so a filtered view compared rows that
    /// were not on screen — and the damage ran the wrong way round. With the
    /// filter keeping `West` over `East/1`(hidden), `West/1`, `West/1`,
    /// `East/2`(hidden), the hidden `East/1` became the *first occurrence*, so
    /// both **visible** rows were deleted as duplicates of a row the user
    /// cannot see, and both hidden rows survived. The grid emptied with no
    /// visible cause.
    ///
    /// Excluding hidden rows only from *deletion* would not fix it: the
    /// invisible row still claims the first occurrence and still takes a
    /// visible one with it. They have to be excluded as keys too.
    #[test]
    fn remove_duplicates_ignores_the_rows_a_filter_is_hiding() {
        session_new();
        for (r, (region, n)) in [
            ("region", "n"),
            ("East", "1"),
            ("West", "1"),
            ("West", "1"),
            ("East", "2"),
        ]
        .iter()
        .enumerate()
        {
            session_set_cell(0, r as u32, 0, region).unwrap();
            session_set_cell(0, r as u32, 1, n).unwrap();
        }
        session_set_filter_range(0, 0, 0, 4, 1).unwrap();
        session_set_filter_values(0, 0, vec!["West".to_owned()]).unwrap();
        assert_eq!(
            filter_hidden(),
            [1, 4],
            "the two East rows are filtered out"
        );

        // Keyed on the **number** column alone, which is what makes the hidden
        // rows collide with the visible ones — the whole point of the defect.
        // Keying on both columns cannot discriminate, because `East` and `West`
        // never produce the same key and the mutation below proves nothing.
        let removed = crate::clipboard::session_remove_duplicates(0, 1, 1, 4, 1).unwrap();
        assert_eq!(removed, 1, "one visible duplicate, not both visible rows");

        let regions: Vec<String> = (0..4).map(|r| shown(r, 0)).collect();
        assert_eq!(
            regions,
            ["region", "East", "West", "East"],
            "both East rows are hidden and must survive; exactly one West goes"
        );
    }

    /// **`DATA-SORT-01`: a sort under a filter still hides the excluded rows.**
    ///
    /// `filter_hidden` is a set of row *indices*, and nothing rebuilds it but
    /// `commit_filter` and a load — so a sort that moved data underneath it left
    /// it pointing at whichever row had arrived at the old index. Asserted
    /// forward, against the sheet as it stands after the sort: a round trip
    /// cannot see this, because stale forward state saves and reloads
    /// symmetrically and comes back exactly as wrong as it went out.
    #[test]
    fn sorting_under_a_filter_keeps_hiding_the_rows_the_filter_excludes() {
        filtered_grid();
        assert_eq!(filter_hidden(), [2], "the East row is the one filtered out");

        // Sort by the *other* column, which is what moves the East row.
        sort_from_filter_menu(1, true);

        // The invariant, stated as the filter states it: a row is hidden if and
        // only if its region is not the one the filter kept.
        for r in 1..=3 {
            let region = shown(r, 0);
            let hidden = with_session(|s| s.workbook().sheets[0].is_row_hidden(r)).unwrap();
            assert_eq!(
                hidden,
                region != "West",
                "row {r} holds {region:?} and hidden={hidden}; the filter keeps West"
            );
        }
        // …and concretely: the East row never moved, because a hidden row is not
        // part of the sort. The two visible rows sorted between themselves.
        assert_eq!(
            (0..4).map(|r| shown(r, 0)).collect::<Vec<_>>(),
            ["region", "West", "East", "West"]
        );
        assert_eq!(
            (0..4).map(|r| shown(r, 1)).collect::<Vec<_>>(),
            ["n", "2", "1", "3"]
        );
        assert_eq!(filter_hidden(), [2]);
    }

    /// **A row hidden by hand is left alone by a sort too.**
    ///
    /// `hidden_rows` and `filter_hidden` are separate fields with separate
    /// reasons, and both are sets of row *indices* that a sort would otherwise
    /// invalidate the same way. `is_row_hidden` is the one predicate that covers
    /// both — a fix that reached only for `filter_hidden` would leave the
    /// hand-hidden half permuted and untracked, which is the case this pins.
    #[test]
    fn a_row_hidden_by_hand_does_not_move_when_the_range_is_sorted() {
        session_new();
        for (r, (a, b)) in [("n", "tag"), ("3", "c"), ("1", "a"), ("2", "b")]
            .iter()
            .enumerate()
        {
            session_set_cell(0, r as u32, 0, a).unwrap();
            session_set_cell(0, r as u32, 1, b).unwrap();
        }
        // No filter at all: the middle row is hidden the way the row header's
        // "Hide rows" hides it.
        session_hide_rows(0, 2, 2).unwrap();
        session_sort_range_multi(0, 1, 0, 3, 1, vec![0], vec![1]).unwrap();

        assert_eq!(
            (0..4).map(|r| shown(r, 0)).collect::<Vec<_>>(),
            ["n", "2", "1", "3"],
            "the hidden row kept its `1`; the two visible rows sorted around it"
        );
        assert_eq!(
            (0..4).map(|r| shown(r, 1)).collect::<Vec<_>>(),
            ["tag", "b", "a", "c"]
        );
        let still: Vec<bool> = with_session(|s| {
            (1..=3)
                .map(|r| s.workbook().sheets[0].is_row_hidden(r))
                .collect()
        })
        .unwrap();
        assert_eq!(still, [false, true, false]);
    }

    /// **Undoing that sort restores the data *and* the hidden set.**
    ///
    /// One Ctrl+Z, not two: the hidden set is part of the same batch as the
    /// move, so the row order and what is hidden can never come back out of
    /// step with each other.
    #[test]
    fn undoing_a_sort_under_a_filter_restores_the_order_and_the_hidden_set() {
        filtered_grid();
        let before: Vec<String> = (0..4).map(|r| shown(r, 1)).collect();
        let hidden_before = hidden_flags();
        assert_eq!(hidden_before, [false, true, false]);

        sort_from_filter_menu(1, true);
        assert_ne!(
            (0..4).map(|r| shown(r, 1)).collect::<Vec<_>>(),
            before,
            "the sort has to change something for the undo to be worth testing"
        );

        session_undo().unwrap();
        assert_eq!((0..4).map(|r| shown(r, 1)).collect::<Vec<_>>(), before);
        assert_eq!(hidden_flags(), hidden_before);
        assert_eq!(filter_hidden(), [2]);
    }
}

/// **Reopening Data ▸ Validation shows the rule that is already there.**
///
/// The panel could read a rule's *wording* back and nothing else, so a cell
/// carrying "whole number between 1 and 10" opened onto empty dropdowns: the
/// user could neither see what they had set nor amend it, only overwrite it
/// blind. What was missing was never model code — `DvKind::ooxml` and
/// `DvOperator::ooxml` have always existed — but a binding that hands the kind,
/// the operator and the operands to the host.
///
/// Asserted through the one call the panel makes, because a field that exists
/// on the struct and is not in the JSON is exactly the shape of this defect.
#[cfg(test)]
mod validation_readback_tests {
    use super::{
        session_clear_validation, session_new, session_set_cell, session_set_list_validation,
        session_set_list_validation_range, session_set_validation, session_validation_messages,
    };

    /// `session_validation_messages` parsed, as the panel parses it.
    fn rule(row: u32, col: u32) -> serde_json::Value {
        let raw = session_validation_messages(0, row, col);
        assert!(!raw.is_empty(), "the cell has a rule");
        serde_json::from_str(&raw).expect("the binding returns json")
    }

    /// **A number rule comes back with its kind, its operator and both
    /// operands.**
    #[test]
    fn a_whole_number_rule_reopens_with_its_operands() {
        session_new();
        session_set_validation(
            0, 0, 0, 0, 0, "whole", "between", "1", "10", true, "1 to 10",
        )
        .unwrap();
        let r = rule(0, 0);
        assert_eq!(r["kind"], "whole");
        assert_eq!(r["operator"], "between");
        assert_eq!(r["formula1"], "1");
        assert_eq!(r["formula2"], "10");
        assert_eq!(r["allowBlank"], true);
        // The wording the panel already read is untouched by the addition.
        assert_eq!(r["errorText"], "1 to 10");
    }

    /// **A one-operand rule reports the operator it was given**, not the
    /// schema's `between` default — the dialog's "Data" dropdown is otherwise
    /// wrong for every rule that is not a range.
    #[test]
    fn a_one_sided_rule_keeps_its_operator_and_leaves_the_second_operand_empty() {
        session_new();
        session_set_validation(0, 0, 0, 0, 0, "textLength", "lessThan", "5", "", false, "")
            .unwrap();
        let r = rule(0, 0);
        assert_eq!(r["kind"], "textLength");
        assert_eq!(r["operator"], "lessThan");
        assert_eq!(r["formula1"], "5");
        assert_eq!(r["formula2"], "");
        assert_eq!(r["allowBlank"], false);
    }

    /// **A list rule says which of its two sources it uses.**
    ///
    /// `session_validation_at` resolves a list to its *options*, which is what
    /// the cell's dropdown needs and not what the dialog needs: reopening has to
    /// show the user the source they typed. A literal list carries `values` and
    /// no `formula1`; a range-backed one carries `formula1` and no `values`, and
    /// the two fill different halves of the dialog.
    #[test]
    fn a_list_rule_reopens_with_the_source_it_was_given() {
        session_new();
        session_set_list_validation(0, 0, 0, 0, 0, vec!["Yes".to_owned(), "No".to_owned()])
            .unwrap();
        let r = rule(0, 0);
        assert_eq!(r["kind"], "list");
        assert_eq!(r["values"], serde_json::json!(["Yes", "No"]));
        assert_eq!(r["formula1"], "");

        session_set_cell(0, 5, 5, "a").unwrap();
        session_set_list_validation_range(0, 1, 0, 1, 0, "$F$1:$F$3").unwrap();
        let r = rule(1, 0);
        assert_eq!(r["kind"], "list");
        assert_eq!(r["formula1"], "$F$1:$F$3");
        assert_eq!(r["values"], serde_json::json!([]));
    }

    /// **A cell with no rule still says so.** The panel keys "is there a rule"
    /// off the empty string, and adding fields must not turn that into a rule
    /// made of defaults — which would be a `none` rule the user never set.
    #[test]
    fn a_cell_without_a_rule_returns_nothing() {
        session_new();
        session_set_validation(0, 0, 0, 0, 0, "whole", "between", "1", "10", true, "").unwrap();
        assert!(!session_validation_messages(0, 0, 0).is_empty());
        session_clear_validation(0, 0, 0, 0, 0).unwrap();
        assert_eq!(session_validation_messages(0, 0, 0), "");
        assert_eq!(session_validation_messages(0, 9, 9), "");
    }
}

/// **Whole-row highlighting, through the bindings the editor actually calls.**
///
/// The engine could hold and evaluate a formula rule and still be unreachable
/// from the product: that gap — "engine ✓ / editor ✗" — is the one
/// `docs/12` counts seven times. So this drives `session_add_cf` with the
/// `formula` kind and reads the grid back through `session_cells`, which is the
/// call the canvas makes for every frame.
#[cfg(test)]
mod formula_cf_tests {
    use super::{cf_rule_from_kind, session_add_cf, session_new, session_set_cell};
    use crate::axis::session_cells;
    use crate::formula::session_cf_rules;

    /// The background `session_cells` reports for one cell, or `""`.
    fn fill_at(row: u32, col: u32) -> String {
        let cells: serde_json::Value =
            serde_json::from_str(&session_cells(0, 0, 0, 20, 20)).expect("json");
        cells
            .as_array()
            .expect("an array")
            .iter()
            .find(|c| c["r"] == row && c["c"] == col)
            .and_then(|c| c["bg"].as_str())
            .unwrap_or_default()
            .to_owned()
    }

    /// A rule over `A2:H10` saying `=$D2>100` paints the rows whose D is over a
    /// hundred, and only those — the anchor being the range's top-left, not
    /// `A1`, which is why the range starts at row 2.
    #[test]
    fn a_formula_rule_highlights_whole_rows() {
        session_new();
        session_set_cell(0, 1, 3, "150").unwrap(); // D2
        session_set_cell(0, 2, 3, "50").unwrap(); // D3
        session_set_cell(0, 3, 3, "900").unwrap(); // D4
        session_add_cf(0, 1, 0, 9, 7, "formula", 0.0, 0.0, "=$D2>100", "FFC7CE").unwrap();

        assert_eq!(fill_at(1, 3), "FFC7CE", "D2's own row is highlighted");
        assert_eq!(fill_at(2, 3), "", "D3's is not");
        assert_eq!(fill_at(3, 3), "FFC7CE", "D4's is");

        // And the rules manager can say what the rule is, rather than showing a
        // rule it has no words for.
        let rules: serde_json::Value = serde_json::from_str(&session_cf_rules(0)).expect("json");
        assert_eq!(rules[0]["desc"], "formula =$D2>100");
        assert_eq!(rules[0]["range"], "A2:H10");
    }

    /// A formula that does not parse is refused at the point of authoring.
    ///
    /// Storing it would produce a rule that can never match and never explains
    /// itself — the user would see no highlight and no reason for it.
    #[test]
    fn a_formula_that_does_not_parse_is_refused() {
        assert!(
            cf_rule_from_kind("formula", 0.0, 0.0, "=$D2>").is_err(),
            "an unparseable formula is not accepted"
        );
        assert!(
            cf_rule_from_kind("formula", 0.0, 0.0, "   ").is_err(),
            "and neither is an empty one"
        );
        assert_eq!(
            cf_rule_from_kind("formula", 0.0, 0.0, " =$D2>100 "),
            Ok(casual_calc_model::CfRule::Expression("$D2>100".to_owned())),
            "a good one loses its `=` and its whitespace and keeps the rest"
        );
    }
}
