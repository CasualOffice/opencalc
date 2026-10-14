//! The workbook's macros (`MACRO-02`, docs/99): list, read, create, edit,
//! rename, delete, and the autostart flag.
//!
//! Every change is one `SetMacros` edit — a whole-library replace — so it is
//! one undo step, travels to collaborators as an ordinary operation, is refused
//! in a read-only session by the engine rather than by the chrome, and marks the
//! workbook edited so the next save writes the macros part. A change that
//! changes nothing is not submitted, so it leaves no empty undo step.
//!
//! Macros are identified by `guid` (ONLYOFFICE's key): names may repeat.

use super::*;
use casual_calc_model::{Macro, MacroLibrary};

/// The library as JSON: `{"macros":[{guid,name,source,autostart}],
/// "customFunctions":[{guid,name,source}]}`. Every key is always present, so
/// the page never has to guess at a default.
#[wasm_bindgen]
pub fn session_macros() -> String {
    with_session(|s| {
        let lib = &s.workbook().macros;
        serde_json::json!({
            "macros": lib.macros.iter().map(|m| serde_json::json!({
                "guid": m.guid, "name": m.name, "source": m.source, "autostart": m.autostart,
            })).collect::<Vec<_>>(),
            "customFunctions": lib.custom_functions.iter().map(|f| serde_json::json!({
                "guid": f.guid, "name": f.name, "source": f.source,
            })).collect::<Vec<_>>(),
        })
        .to_string()
    })
    .unwrap_or_else(|| r#"{"macros":[],"customFunctions":[]}"#.to_owned())
}

/// One macro's source, or `None` when no macro has that guid.
#[wasm_bindgen]
pub fn session_macro_source(guid: &str) -> Option<String> {
    with_session(|s| s.workbook().macros.get(guid).map(|m| m.source.clone())).flatten()
}

/// Whether any macro asks to run when the workbook opens — the question the
/// editor asks before offering to run them.
#[wasm_bindgen]
pub fn session_has_autostart_macros() -> bool {
    with_session(|s| s.workbook().macros.has_autostart()).unwrap_or(false)
}

/// Whether the macro with this guid asks to run on open.
#[wasm_bindgen]
pub fn session_macro_autostart(guid: &str) -> bool {
    with_session(|s| s.workbook().macros.get(guid).is_some_and(|m| m.autostart)).unwrap_or(false)
}

/// Apply `change` to a copy of the library and submit it as one edit, unless
/// it changed nothing.
fn edit_library(
    change: impl FnOnce(&mut MacroLibrary) -> Result<(), JsError>,
) -> Result<(), JsError> {
    with_session_mut(|session| {
        let mut next = session.workbook().macros.clone();
        change(&mut next)?;
        if next == session.workbook().macros {
            return Ok(());
        }
        next.check().map_err(js)?;
        session.edit(EditOperation::SetMacros(next)).map_err(js)
    })
}

fn find<'a>(lib: &'a mut MacroLibrary, guid: &str) -> Result<&'a mut Macro, JsError> {
    lib.macros
        .iter_mut()
        .find(|m| m.guid == guid)
        .ok_or_else(|| JsError::new("no macro with that id"))
}

/// Create the macro `guid`, or replace its name, source and flag if it exists.
/// A new macro goes to the end of the list.
#[wasm_bindgen]
pub fn session_set_macro(
    guid: &str,
    name: &str,
    source: &str,
    autostart: bool,
) -> Result<(), JsError> {
    edit_library(|lib| {
        let entry = Macro {
            guid: guid.to_owned(),
            name: name.trim().to_owned(),
            source: source.to_owned(),
            autostart,
        };
        match lib.macros.iter_mut().find(|m| m.guid == guid) {
            Some(existing) => *existing = entry,
            None => lib.macros.push(entry),
        }
        Ok(())
    })
}

/// Replace one macro's source.
#[wasm_bindgen]
pub fn session_set_macro_source(guid: &str, source: &str) -> Result<(), JsError> {
    edit_library(|lib| {
        find(lib, guid)?.source = source.to_owned();
        Ok(())
    })
}

/// Rename one macro.
#[wasm_bindgen]
pub fn session_rename_macro(guid: &str, name: &str) -> Result<(), JsError> {
    edit_library(|lib| {
        find(lib, guid)?.name = name.trim().to_owned();
        Ok(())
    })
}

/// Turn one macro's run-on-open request on or off.
#[wasm_bindgen]
pub fn session_set_macro_autostart(guid: &str, on: bool) -> Result<(), JsError> {
    edit_library(|lib| {
        find(lib, guid)?.autostart = on;
        Ok(())
    })
}

/// Delete one macro.
#[wasm_bindgen]
pub fn session_delete_macro(guid: &str) -> Result<(), JsError> {
    edit_library(|lib| {
        let before = lib.macros.len();
        lib.macros.retain(|m| m.guid != guid);
        if lib.macros.len() == before {
            return Err(JsError::new("no macro with that id"));
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() {
        session_new();
    }

    fn names() -> Vec<String> {
        with_session(|s| {
            s.workbook()
                .macros
                .macros
                .iter()
                .map(|m| m.name.clone())
                .collect()
        })
        .unwrap()
    }

    #[test]
    fn create_edit_rename_toggle_delete_each_one_undo_step() {
        fresh();
        session_set_macro("g1", "Fill", "Api.GetActiveSheet();", false).unwrap();
        session_set_macro("g2", "Tidy", "// tidy", false).unwrap();
        assert_eq!(names(), ["Fill", "Tidy"]);
        session_rename_macro("g1", " Fill all ").unwrap();
        session_set_macro_source("g1", "1+1;").unwrap();
        session_set_macro_autostart("g2", true).unwrap();
        assert!(session_has_autostart_macros());
        assert!(session_macro_autostart("g2"));
        assert_eq!(session_macro_source("g1").as_deref(), Some("1+1;"));
        assert_eq!(names(), ["Fill all", "Tidy"]);
        session_delete_macro("g2").unwrap();
        assert_eq!(names(), ["Fill all"]);
        assert!(!session_has_autostart_macros());

        // Each change above was one step: undo walks them back one at a time.
        with_session_mut(|s| s.undo().map(|_| ()).map_err(js)).unwrap();
        assert_eq!(
            names(),
            ["Fill all", "Tidy"],
            "undo brings the deleted macro back"
        );
        let json: serde_json::Value = serde_json::from_str(&session_macros()).unwrap();
        assert_eq!(json["macros"][1]["autostart"], true);
        assert_eq!(json["customFunctions"], serde_json::json!([]));
    }

    #[test]
    fn a_change_that_changes_nothing_leaves_no_undo_step() {
        fresh();
        session_set_macro("g1", "Fill", "x", false).unwrap();
        let before = with_session(|s| s.can_undo()).unwrap();
        assert!(before);
        session_set_macro_source("g1", "x").unwrap();
        with_session_mut(|s| s.undo().map(|_| ()).map_err(js)).unwrap();
        assert!(
            names().is_empty(),
            "the no-op was not a step, so one undo removed the macro"
        );
    }

    #[test]
    fn a_read_only_session_refuses_macro_edits() {
        fresh();
        session_set_read_only(true);
        // Through the engine the binding submits to (a `JsError` cannot be
        // built off-wasm): the refusal is the engine's, not the chrome's.
        let mut lib = MacroLibrary::default();
        lib.macros.push(Macro {
            guid: "g1".into(),
            name: "Fill".into(),
            source: "x".into(),
            autostart: false,
        });
        let refused = SESSION.with(|c| {
            c.borrow_mut()
                .as_mut()
                .unwrap()
                .edit(EditOperation::SetMacros(lib))
                .is_err()
        });
        assert!(refused);
        session_set_read_only(false);
        assert!(names().is_empty());
    }
}
