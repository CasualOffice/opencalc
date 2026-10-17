//! Custom worksheet functions (`MACRO-04`): the page's half of the async
//! model.
//!
//! The editor runs the workbook's custom-function sources in a sandboxed
//! worker, registers what they declared here, takes the calls a recalculation
//! wanted, runs them in the worker, and hands the answers back. None of it is
//! an edit: it is not recorded, not sent to collaborators, and not undoable —
//! each client computes its own answers (see `casual-calc-model`
//! `CustomFunctionRuntime`).
//!
//! Values cross as JSON: a number, a string, a boolean, `null` for empty,
//! `{"error":"#N/A"}` for an error, and a block as an array of rows.
//! Everything coming back from the page is untrusted — it came out of a
//! sandbox running workbook code — so it is bounded before it is used.

use super::*;
use casual_calc_model::{
    CustomFunctionSpec, CustomParam, CustomValue, ErrorValue, MAX_CUSTOM_PARAMS,
};
use serde_json::{Value as Json, json};

/// The largest JSON the page may hand back in one call.
const MAX_JSON_BYTES: usize = 64 << 20;

fn parse_json(text: &str) -> Result<Json, JsError> {
    if text.len() > MAX_JSON_BYTES {
        return Err(JsError::new("too large"));
    }
    serde_json::from_str(text).map_err(js)
}

/// Register the workbook's custom functions, replacing any registered before.
///
/// `json`: `[{name, version, minArgs, maxArgs, volatile, description,
/// params: [{name, type, optional, description}], returns}]`. Every formula is
/// recalculated: a cell that was `#NAME?` now asks for its function.
#[wasm_bindgen]
pub fn session_set_custom_functions(json: &str) -> Result<(), JsError> {
    let list = parse_json(json)?;
    let list = list
        .as_array()
        .ok_or_else(|| JsError::new("expected a list of functions"))?;
    let text = |v: &Json, key: &str| v.get(key).and_then(Json::as_str).unwrap_or("").to_owned();
    let count = |v: &Json, key: &str| {
        v.get(key)
            .and_then(Json::as_u64)
            .map_or(0, |n| n.min(MAX_CUSTOM_PARAMS as u64 + 1) as usize)
    };
    let specs = list
        .iter()
        .map(|f| CustomFunctionSpec {
            name: text(f, "name"),
            version: text(f, "version"),
            min_args: count(f, "minArgs"),
            max_args: count(f, "maxArgs"),
            volatile: f.get("volatile").and_then(Json::as_bool).unwrap_or(false),
            description: text(f, "description"),
            params: f
                .get("params")
                .and_then(Json::as_array)
                .map(|ps| {
                    ps.iter()
                        .take(MAX_CUSTOM_PARAMS + 1)
                        .map(|p| CustomParam {
                            name: text(p, "name"),
                            kind: text(p, "type"),
                            optional: p.get("optional").and_then(Json::as_bool).unwrap_or(false),
                            description: text(p, "description"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            returns: text(f, "returns"),
        })
        .collect();
    with_session_mut(|s| s.set_custom_functions(specs).map_err(js))
}

/// Forget every custom-function answer and ask again (`MACRO-19`): Excel's
/// Enable Content recomputes what the unblocked content feeds.
#[wasm_bindgen]
pub fn session_refresh_custom_functions() {
    let _ = with_session_mut(|s| {
        s.refresh_custom_functions();
        Ok(())
    });
}

/// The registered functions, for Insert Function, the argument hint and
/// autocomplete: `[{name, description, params:[{name,type,optional,description}],
/// returns, volatile, minArgs, maxArgs}]`, by name.
#[wasm_bindgen]
pub fn session_custom_functions() -> String {
    with_session(|s| {
        Json::Array(
            s.workbook()
                .custom
                .functions()
                .map(|f| {
                    json!({
                        "name": f.name,
                        "description": f.description,
                        "params": f.params.iter().map(|p| json!({
                            "name": p.name, "type": p.kind,
                            "optional": p.optional, "description": p.description,
                        })).collect::<Vec<_>>(),
                        "returns": f.returns,
                        "volatile": f.volatile,
                        "minArgs": f.min_args,
                        "maxArgs": f.max_args,
                    })
                })
                .collect(),
        )
        .to_string()
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Cheap enough to ask after every draw: `{instance, generation, pending,
/// inFlight, registered}`. `instance` changes when the workbook is replaced
/// (an open, File ▸ New, a collaboration join) and `generation` when its
/// custom-function sources change — either means register again.
#[wasm_bindgen]
pub fn session_custom_status() -> String {
    with_session(|s| {
        let rt = &s.workbook().custom;
        json!({
            "instance": rt.instance(),
            "generation": rt.library_generation(),
            "pending": rt.pending_len(),
            "inFlight": rt.in_flight_len(),
            "registered": rt.functions().count(),
        })
        .to_string()
    })
    .unwrap_or_else(|| "null".to_owned())
}

fn to_json(v: &CustomValue) -> Json {
    match v {
        CustomValue::Empty => Json::Null,
        CustomValue::Number(n) => json!(n),
        CustomValue::Text(s) => json!(s),
        CustomValue::Bool(b) => json!(b),
        CustomValue::Error(e) => json!({ "error": e.to_string() }),
        CustomValue::Array { cols, cells, .. } => Json::Array(
            cells
                .chunks((*cols).max(1))
                .map(|row| Json::Array(row.iter().map(to_json).collect()))
                .collect(),
        ),
    }
}

/// Take the calls recalculation wanted: `[{ticket, name, address, args,
/// argAddresses}]`. Each is handed out once; answer it with
/// [`session_set_custom_results`].
#[wasm_bindgen]
pub fn session_custom_calls() -> Result<String, JsError> {
    with_session_mut(|s| {
        let calls = s.take_custom_calls();
        Ok(Json::Array(
            calls
                .iter()
                .map(|c| {
                    json!({
                        "ticket": c.ticket,
                        "name": c.key.name,
                        "address": c.key.address,
                        "args": c.key.args.iter().map(to_json).collect::<Vec<_>>(),
                        "argAddresses": c.key.arg_addresses,
                    })
                })
                .collect(),
        )
        .to_string())
    })
}

fn error_token(token: &str) -> ErrorValue {
    match token {
        "#NULL!" => ErrorValue::Null,
        "#DIV/0!" | "#DIV/0" => ErrorValue::Div0,
        "#REF!" => ErrorValue::Ref,
        "#NAME?" => ErrorValue::Name,
        "#NUM!" => ErrorValue::Num,
        "#N/A" => ErrorValue::Na,
        "#SPILL!" => ErrorValue::Spill,
        "#CALC!" => ErrorValue::Calc,
        "#BUSY!" => ErrorValue::Busy,
        _ => ErrorValue::Value,
    }
}

fn scalar_from(v: &Json) -> CustomValue {
    match v {
        Json::Null => CustomValue::Empty,
        Json::Bool(b) => CustomValue::Bool(*b),
        Json::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() => CustomValue::Number(f),
            _ => CustomValue::Error(ErrorValue::Num),
        },
        Json::String(s) => CustomValue::Text(s.clone()),
        Json::Object(o) => match o.get("error").and_then(Json::as_str) {
            Some(token) => CustomValue::Error(error_token(token)),
            None => CustomValue::Error(ErrorValue::Value),
        },
        Json::Array(_) => CustomValue::Error(ErrorValue::Value),
    }
}

/// An answer: a scalar, or a block of equal-length rows. A ragged or empty
/// block is `#VALUE!`; one wider or taller than the grid keeps its shape (with
/// no cells) so the evaluator refuses it as `#NUM!` without it being built. The
/// cell count is bounded by the JSON's size and then by the evaluator's array
/// bound.
fn answer_from(v: &Json) -> CustomValue {
    let Json::Array(rows) = v else {
        return scalar_from(v);
    };
    // ONLYOFFICE takes a one-dimensional list as a single row.
    let rows: Vec<&Json> = if rows.iter().all(|r| !r.is_array()) {
        vec![v]
    } else {
        rows.iter().collect()
    };
    let cols = rows.first().and_then(|r| r.as_array()).map_or(0, Vec::len);
    if rows.is_empty() || cols == 0 {
        return CustomValue::Error(ErrorValue::Value);
    }
    if rows.len() > casual_calc_model::GRID_MAX_ROW as usize + 1
        || cols > casual_calc_model::GRID_MAX_COL as usize + 1
    {
        return CustomValue::Array {
            rows: rows.len(),
            cols,
            cells: Vec::new(),
        };
    }
    let mut cells = Vec::with_capacity(rows.len() * cols);
    for r in &rows {
        match r.as_array() {
            Some(row) if row.len() == cols => {
                for c in row {
                    cells.push(match c {
                        Json::Array(_) => return CustomValue::Error(ErrorValue::Value),
                        other => scalar_from(other),
                    });
                }
            }
            _ => return CustomValue::Error(ErrorValue::Value),
        }
    }
    CustomValue::Array {
        rows: rows.len(),
        cols,
        cells,
    }
}

/// Hand back answers, `[{ticket, value}]`, and recalculate the cells that
/// asked and everything that reads them. Returns how many tickets were
/// recognised; a stale one is ignored.
#[wasm_bindgen]
pub fn session_set_custom_results(json: &str) -> Result<u32, JsError> {
    let list = parse_json(json)?;
    let list = list
        .as_array()
        .ok_or_else(|| JsError::new("expected a list of results"))?;
    let results = list
        .iter()
        .filter_map(|r| {
            let ticket = r.get("ticket")?.as_u64()?;
            Some((ticket, answer_from(r.get("value").unwrap_or(&Json::Null))))
        })
        .collect();
    with_session_mut(|s| Ok(s.set_custom_results(results) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_strictly() {
        assert_eq!(answer_from(&json!(42)), CustomValue::Number(42.0));
        assert_eq!(
            answer_from(&json!({"error": "#N/A"})),
            CustomValue::Error(ErrorValue::Na)
        );
        assert_eq!(
            answer_from(&json!([[1, 2], [3]])),
            CustomValue::Error(ErrorValue::Value),
            "ragged"
        );
        assert_eq!(
            answer_from(&json!([[1, [2]]])),
            CustomValue::Error(ErrorValue::Value),
            "nested"
        );
        assert_eq!(
            answer_from(&json!([1, 2, 3])),
            CustomValue::Array {
                rows: 1,
                cols: 3,
                cells: vec![
                    CustomValue::Number(1.0),
                    CustomValue::Number(2.0),
                    CustomValue::Number(3.0)
                ]
            },
            "a flat list is one row, as ONLYOFFICE reads it"
        );
    }
}
