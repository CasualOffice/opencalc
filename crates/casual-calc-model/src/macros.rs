//! Macros kept in the workbook (`MACRO-02`, docs/99 §4).
//!
//! A macro here is JavaScript written against ONLYOFFICE's Office API — the
//! model docs/99 adopted — and it lives **in the document**, as ONLYOFFICE
//! keeps it, so a workbook carries its macros between the two programs.
//!
//! # The file format is ONLYOFFICE's, read and written as it is
//!
//! ONLYOFFICE stores a document's macros in one package part,
//! `xl/jsaProject.bin`, reached from `xl/workbook.xml` by a relationship of
//! type `http://schemas.onlyoffice.com/jsaProject`. Despite the extension the
//! part is UTF-8 JSON — the string its editor holds in `CDocumentMacros.Data`:
//!
//! ```json
//! {"macrosArray":[{"guid":"…","name":"Macro 1","autostart":false,"value":"(function(){…})();"}],
//!  "current":0,
//!  "customFunctions":{"macrosArray":[{"guid":"…","name":"Function 1","value":"…"}]}}
//! ```
//!
//! Sources: `ONLYOFFICE/core` `OOXML/DocxFormat/FileTypes.cpp` (the part name
//! and relationship type), `OOXML/Binary/Sheets/Writer/BinaryReaderS.cpp` (the
//! part goes under `xl/` and is added to the workbook's relationships),
//! `ONLYOFFICE/sdkjs` `common/macros.js` (`macrosArray`, `guid`, `name`,
//! `value`, `autostart`), `cell/model/Serialize.js` (the bytes are read as
//! UTF-8 text; custom functions travel under `customFunctions`), and
//! `ONLYOFFICE/web-apps` `apps/common/main/lib/view/MacrosDialog.js` (what the
//! Macros window writes back, `current` included).
//!
//! The model below is that data with names a reader of this crate can follow:
//! `value` is [`Macro::source`]. `current` — the row ONLYOFFICE's window had
//! selected — is window state rather than document content and is not kept;
//! the writer emits the first row, which is what ONLYOFFICE selects when the
//! field is absent anyway.
//!
//! # Bounds
//!
//! The JSON comes from a file, and over collaboration from a peer: both are
//! untrusted. [`MacroLibrary::check`] bounds the count, the name and the source
//! size, and every entry point — import, the wire, the editor — goes through it.

use serde::{Deserialize, Serialize};

/// The most macros (and, separately, custom functions) one workbook carries.
pub const MAX_MACROS: usize = 1_000;
/// The longest macro or function name, in characters.
pub const MAX_MACRO_NAME_CHARS: usize = 255;
/// The largest single macro source, in bytes.
pub const MAX_MACRO_SOURCE_BYTES: usize = 1 << 20;
/// The largest whole library, in bytes of source.
pub const MAX_LIBRARY_SOURCE_BYTES: usize = 8 << 20;

/// The relationship type ONLYOFFICE gives the macros part.
pub const JSA_PROJECT_REL_TYPE: &str = "http://schemas.onlyoffice.com/jsaProject";
/// Where an `.xlsx` carries it.
pub const JSA_PROJECT_PART: &str = "xl/jsaProject.bin";
/// The content type ONLYOFFICE declares for it (`sdkjs` `common/openxml.js`).
pub const JSA_PROJECT_CONTENT_TYPE: &str = "application/octet-stream";

/// One macro: a named script, and whether it asks to run when the workbook
/// opens.
///
/// `deny_unknown_fields` for the reason [`crate::DefinedName`] has it: the
/// library crosses the collaboration wire as a whole-list replace, and a peer
/// that skipped a field would write the list back without it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Macro {
    /// Stable identity, ONLYOFFICE's `guid`. Names may repeat; this does not.
    pub guid: String,
    /// The name shown in the Macros window.
    pub name: String,
    /// The JavaScript, ONLYOFFICE's `value`.
    pub source: String,
    /// Asks to run on open. Only ever a request: the editor asks the person
    /// first (docs/99 §3, "Trust").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub autostart: bool,
}

/// A custom-function library (`MACRO-04`): a script that registers worksheet
/// functions through `Api.AddCustomFunction`. Kept in its own list, as
/// ONLYOFFICE keeps it, so the storage need not change when they run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomFunctionSource {
    /// Stable identity.
    pub guid: String,
    /// The name shown in the Macros window.
    pub name: String,
    /// The JavaScript.
    pub source: String,
}

/// A workbook's macros and custom-function sources.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroLibrary {
    /// Macros, in the order the Macros window lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macros: Vec<Macro>,
    /// Custom-function sources (`MACRO-04`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_functions: Vec<CustomFunctionSource>,
}

/// Why a library was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroError {
    /// The part is not the JSON ONLYOFFICE writes.
    Malformed(String),
    /// Over one of the bounds above.
    TooLarge(&'static str),
    /// An empty or repeated guid, or an empty name.
    Invalid(&'static str),
}

impl core::fmt::Display for MacroError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "the macros part is not readable: {why}"),
            Self::TooLarge(what) => write!(f, "too large: {what}"),
            Self::Invalid(what) => write!(f, "invalid: {what}"),
        }
    }
}

impl std::error::Error for MacroError {}

impl MacroLibrary {
    /// Whether the workbook carries no macros and no function sources.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.macros.is_empty() && self.custom_functions.is_empty()
    }

    /// Whether any macro asks to run when the workbook opens.
    #[must_use]
    pub fn has_autostart(&self) -> bool {
        self.macros.iter().any(|m| m.autostart)
    }

    /// The macro with this guid.
    #[must_use]
    pub fn get(&self, guid: &str) -> Option<&Macro> {
        self.macros.iter().find(|m| m.guid == guid)
    }

    /// Check the bounds and identities every entry point enforces.
    ///
    /// # Errors
    ///
    /// [`MacroError::TooLarge`] over a bound, [`MacroError::Invalid`] for an
    /// empty or repeated guid or an empty name.
    pub fn check(&self) -> Result<(), MacroError> {
        if self.macros.len() > MAX_MACROS || self.custom_functions.len() > MAX_MACROS {
            return Err(MacroError::TooLarge(
                "more macros than a workbook may carry",
            ));
        }
        let entries = self
            .macros
            .iter()
            .map(|m| (&m.guid, &m.name, &m.source))
            .chain(
                self.custom_functions
                    .iter()
                    .map(|f| (&f.guid, &f.name, &f.source)),
            );
        let mut total = 0usize;
        let mut guids = std::collections::BTreeSet::new();
        for (guid, name, source) in entries {
            if guid.is_empty() || guid.chars().count() > MAX_MACRO_NAME_CHARS {
                return Err(MacroError::Invalid("a macro needs a guid"));
            }
            if !guids.insert(guid.as_str()) {
                return Err(MacroError::Invalid("two macros share a guid"));
            }
            if name.trim().is_empty() {
                return Err(MacroError::Invalid("a macro needs a name"));
            }
            if name.chars().count() > MAX_MACRO_NAME_CHARS {
                return Err(MacroError::TooLarge("a macro name"));
            }
            if source.len() > MAX_MACRO_SOURCE_BYTES {
                return Err(MacroError::TooLarge("a macro's source"));
            }
            total += source.len();
        }
        if total > MAX_LIBRARY_SOURCE_BYTES {
            return Err(MacroError::TooLarge("the macros' combined source"));
        }
        Ok(())
    }

    /// The custom functions the sources declare, upper-case, in the order
    /// written — read from the text, **not** by running it.
    ///
    /// For naming things only (an import or save report that says
    /// "custom function DOUBLE" rather than "unsupported function DOUBLE"):
    /// what is actually registered is decided by running the sources in the
    /// sandbox (`MACRO-04`). Recognised: `Api.AddCustomFunction(function NAME`
    /// (optionally `async`), and `Api.AddCustomFunction(NAME)` naming a
    /// function declared in the same source.
    #[must_use]
    pub fn declared_custom_function_names(&self) -> Vec<String> {
        const CALL: &str = "AddCustomFunction(";
        fn ident(text: &str) -> &str {
            let end = text
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(text.len());
            &text[..end]
        }
        let mut out: Vec<String> = Vec::new();
        for source in self.custom_functions.iter().map(|f| f.source.as_str()) {
            let mut rest = source;
            while let Some(at) = rest.find(CALL) {
                rest = &rest[at + CALL.len()..];
                let mut arg = rest.trim_start();
                if let Some(after) = arg.strip_prefix("async") {
                    arg = after.trim_start();
                }
                let name = if let Some(after) = arg.strip_prefix("function") {
                    ident(after.trim_start_matches(|c: char| c.is_whitespace() || c == '*'))
                } else {
                    let named = ident(arg);
                    let declared = !named.is_empty()
                        && source.match_indices("function").any(|(i, _)| {
                            let after = source[i + "function".len()..].trim_start();
                            ident(after) == named
                        });
                    if declared { named } else { "" }
                };
                let upper = name.to_ascii_uppercase();
                if !upper.is_empty() && !out.contains(&upper) {
                    out.push(upper);
                }
            }
        }
        out
    }

    /// Read ONLYOFFICE's `jsaProject.bin` (see the module docs).
    ///
    /// Lenient where ONLYOFFICE is — a missing `guid` is given one, derived
    /// from the position so the same file always reads the same; a missing
    /// `autostart` is off; keys it does not write are ignored — and strict
    /// about the shape and the bounds.
    ///
    /// # Errors
    ///
    /// [`MacroError::Malformed`] when the bytes are not that JSON, or the
    /// bound [`check`](Self::check) refuses.
    pub fn from_jsa_project(bytes: &[u8]) -> Result<Self, MacroError> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        if bytes.len() > MAX_LIBRARY_SOURCE_BYTES * 2 {
            return Err(MacroError::TooLarge("the macros part"));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| MacroError::Malformed("not UTF-8".to_owned()))?;
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| MacroError::Malformed(e.to_string()))?;
        let root = value
            .as_object()
            .ok_or_else(|| MacroError::Malformed("not a JSON object".to_owned()))?;
        let entries = |holder: Option<&serde_json::Value>,
                       kind: &str|
         -> Result<Vec<(String, String, String, bool)>, MacroError> {
            let Some(holder) = holder else {
                return Ok(Vec::new());
            };
            if holder.is_null() {
                return Ok(Vec::new());
            }
            let list = holder
                .as_array()
                .ok_or_else(|| MacroError::Malformed("macrosArray is not a list".to_owned()))?;
            if list.len() > MAX_MACROS {
                return Err(MacroError::TooLarge(
                    "more macros than a workbook may carry",
                ));
            }
            list.iter()
                .enumerate()
                .map(|(i, item)| {
                    let item = item.as_object().ok_or_else(|| {
                        MacroError::Malformed("a macro is not an object".to_owned())
                    })?;
                    let text = |key: &str| item.get(key).and_then(serde_json::Value::as_str);
                    let source = text("value")
                        .ok_or_else(|| MacroError::Malformed("a macro has no value".to_owned()))?;
                    let name = text("name")
                        .filter(|n| !n.trim().is_empty())
                        .map_or_else(|| format!("{kind} {}", i + 1), str::to_owned);
                    let guid = text("guid")
                        .filter(|g| !g.is_empty())
                        .map_or_else(|| format!("{kind}-{}", i + 1), str::to_owned);
                    let autostart = item
                        .get("autostart")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    Ok((guid, name, source.to_owned(), autostart))
                })
                .collect()
        };
        let macros = entries(root.get("macrosArray"), "Macro")?
            .into_iter()
            .map(|(guid, name, source, autostart)| Macro {
                guid,
                name,
                source,
                autostart,
            })
            .collect();
        let functions = root
            .get("customFunctions")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_object().ok_or_else(|| {
                    MacroError::Malformed("customFunctions is not an object".to_owned())
                })
            })
            .transpose()?;
        let custom_functions = entries(functions.and_then(|f| f.get("macrosArray")), "Function")?
            .into_iter()
            .map(|(guid, name, source, _)| CustomFunctionSource { guid, name, source })
            .collect();
        let library = Self {
            macros,
            custom_functions,
        };
        library.check()?;
        Ok(library)
    }

    /// Write ONLYOFFICE's `jsaProject.bin`: the JSON its editor reads back.
    #[must_use]
    pub fn to_jsa_project(&self) -> String {
        use serde_json::json;
        let macros: Vec<serde_json::Value> = self
            .macros
            .iter()
            .map(|m| {
                json!({
                    "guid": m.guid,
                    "name": m.name,
                    "autostart": m.autostart,
                    "value": m.source,
                })
            })
            .collect();
        let current: i64 = if macros.is_empty() { -1 } else { 0 };
        let mut root = json!({ "macrosArray": macros, "current": current });
        if !self.custom_functions.is_empty() {
            let functions: Vec<serde_json::Value> = self
                .custom_functions
                .iter()
                .map(|f| json!({ "guid": f.guid, "name": f.name, "value": f.source }))
                .collect();
            root["customFunctions"] = json!({ "macrosArray": functions });
        }
        root.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(name: &str, autostart: bool) -> Macro {
        Macro {
            guid: format!("g-{name}"),
            name: name.to_owned(),
            source: format!("Api.GetActiveSheet().GetRange(\"A1\").SetValue(\"{name}\");"),
            autostart,
        }
    }

    /// What ONLYOFFICE's Macros window writes (`MacrosDialog.js` `_handleInput`
    /// plus `Serialize.js` folding the custom functions in) reads as the model,
    /// and writing it back gives JSON ONLYOFFICE reads the same way.
    #[test]
    fn onlyoffice_json_reads_and_writes_back() {
        let written = r#"{"macrosArray":[{"guid":"7a1f","name":"Macro 1","autostart":true,"value":"(function()\n{\n\tApi.GetActiveSheet().GetRange(\"B2\").SetValue(42);\n})();"},{"guid":"7a20","name":"Tidy","autostart":false,"value":"x"}],"current":1,"customFunctions":{"macrosArray":[{"guid":"cf1","name":"Function 1","value":"Api.AddCustomFunction(function ADD(a,b){return a+b;});"}]}}"#;
        let lib = MacroLibrary::from_jsa_project(written.as_bytes()).expect("reads");
        assert_eq!(lib.macros.len(), 2);
        assert_eq!(lib.macros[0].name, "Macro 1");
        assert!(lib.macros[0].autostart);
        assert!(lib.macros[0].source.contains("SetValue(42)"));
        assert_eq!(lib.custom_functions[0].name, "Function 1");
        let again = MacroLibrary::from_jsa_project(lib.to_jsa_project().as_bytes()).unwrap();
        assert_eq!(again, lib);
        let v: serde_json::Value = serde_json::from_str(&lib.to_jsa_project()).unwrap();
        assert_eq!(v["macrosArray"][0]["value"], lib.macros[0].source.as_str());
        assert_eq!(v["macrosArray"][1]["autostart"], false);
        assert_eq!(v["current"], 0);
    }

    #[test]
    fn a_bom_and_missing_optional_keys_are_tolerated() {
        let lib =
            MacroLibrary::from_jsa_project(b"\xEF\xBB\xBF{\"macrosArray\":[{\"value\":\"1\"}]}")
                .unwrap();
        assert_eq!(
            lib.macros,
            vec![Macro {
                guid: "Macro-1".into(),
                name: "Macro 1".into(),
                source: "1".into(),
                autostart: false
            }]
        );
    }

    #[test]
    fn malformed_and_oversized_parts_are_refused() {
        assert!(matches!(
            MacroLibrary::from_jsa_project(b"not json"),
            Err(MacroError::Malformed(_))
        ));
        assert!(matches!(
            MacroLibrary::from_jsa_project(br#"{"macrosArray":[{"name":"x"}]}"#),
            Err(MacroError::Malformed(_))
        ));
        let big = MacroLibrary {
            macros: vec![Macro {
                source: "x".repeat(MAX_MACRO_SOURCE_BYTES + 1),
                ..one("a", false)
            }],
            custom_functions: vec![],
        };
        assert_eq!(big.check(), Err(MacroError::TooLarge("a macro's source")));
        let twins = MacroLibrary {
            macros: vec![
                one("a", false),
                Macro {
                    name: "b".into(),
                    ..one("a", false)
                },
            ],
            custom_functions: vec![],
        };
        assert_eq!(
            twins.check(),
            Err(MacroError::Invalid("two macros share a guid"))
        );
    }

    #[test]
    fn autostart_is_skipped_on_the_model_wire_when_off() {
        let json = serde_json::to_string(&one("a", false)).unwrap();
        assert!(!json.contains("autostart"), "{json}");
        assert!(MacroLibrary::default().is_empty());
        assert!(
            MacroLibrary {
                macros: vec![one("a", true)],
                custom_functions: vec![]
            }
            .has_autostart()
        );
    }
}
