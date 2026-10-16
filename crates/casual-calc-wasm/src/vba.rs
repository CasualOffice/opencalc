//! The read-only VBA viewer (`MACRO-05`, docs/99 §4): the VBA project a
//! macro-enabled workbook carries, decoded for **reading only**. Nothing here
//! runs VBA, edits it, or touches the bytes the next save writes.

use super::*;

/// Whether the open workbook carries a VBA project — the question the
/// Developer tab asks before offering **View VBA**. True for a project that
/// will not decode too: the dialog then says why.
#[wasm_bindgen]
pub fn session_has_vba_project() -> bool {
    with_session(|s| s.workbook().macro_project().is_some()).unwrap_or(false)
}

/// The decoded VBA project as JSON:
///
/// `{"present":bool, "error":string|null, "name":string, "codePage":number,
///   "codePageSupported":bool,
///   "modules":[{"name","kind","source":string|null,"error":string|null}]}`
///
/// `kind` is `standard`, `class`, `document` or `designer`. A project that
/// will not decode has `error` set and no modules; a module that will not
/// decode has its own `error` and a null `source`, and the others still read.
/// Every key is always present.
#[wasm_bindgen]
pub fn session_vba_modules() -> String {
    let absent = || {
        serde_json::json!({
            "present": false, "error": null, "name": "", "codePage": 0,
            "codePageSupported": true, "modules": [],
        })
    };
    let value = with_session(|s| match s.vba_project() {
        None => absent(),
        Some(Err(e)) => serde_json::json!({
            "present": true, "error": e.to_string(), "name": "", "codePage": 0,
            "codePageSupported": true, "modules": [],
        }),
        Some(Ok(p)) => serde_json::json!({
            "present": true,
            "error": null,
            "name": p.name,
            "codePage": p.code_page,
            "codePageSupported": p.code_page_supported,
            "modules": p.modules.iter().map(|m| serde_json::json!({
                "name": m.name,
                "kind": m.kind.as_str(),
                "source": m.source.as_ref().ok(),
                "error": m.source.as_ref().err().map(ToString::to_string),
            })).collect::<Vec<_>>(),
        }),
    })
    .unwrap_or_else(absent);
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::{session_has_vba_project, session_vba_modules, set_session};
    use casual_calc_sdk::{SessionFormat, WorkbookSession};

    const HELLO_XLSM: &[u8] = include_bytes!("../../../tests/fixtures/vba-hello.xlsm");

    #[test]
    fn the_binding_lists_modules_with_their_source() {
        set_session(
            WorkbookSession::open_as(
                HELLO_XLSM.to_vec(),
                SessionFormat::for_extension("xlsm").unwrap(),
            )
            .unwrap(),
        );
        assert!(session_has_vba_project());
        let v: serde_json::Value = serde_json::from_str(&session_vba_modules()).unwrap();
        assert_eq!(v["present"], true);
        assert_eq!(v["error"], serde_json::Value::Null);
        assert_eq!(v["codePage"], 1252);
        let modules = v["modules"].as_array().unwrap();
        let summary: Vec<(String, String)> = modules
            .iter()
            .map(|m| {
                (
                    m["name"].as_str().unwrap().to_owned(),
                    m["kind"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("ThisWorkbook".into(), "document".into()),
                ("Module1".into(), "standard".into()),
                ("Class1".into(), "class".into()),
            ]
        );
        assert!(
            modules[1]["source"]
                .as_str()
                .unwrap()
                .contains("Sub Hello()")
        );

        set_session(WorkbookSession::blank());
        assert!(!session_has_vba_project());
        let v: serde_json::Value = serde_json::from_str(&session_vba_modules()).unwrap();
        assert_eq!(v["present"], false);
    }
}
