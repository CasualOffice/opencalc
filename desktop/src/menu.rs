//! The editor's menu, translated for a native menu bar.
//!
//! A desktop app should behave like a desktop app: the operating system draws
//! the menu bar, not an HTML strip inside the window. The webview hands over
//! [`menuModel()`] — the File/Edit/View tree derived from its own live DOM — and
//! this turns it into something a platform menu builder can consume, with the
//! ids the webview dispatches by.
//!
//! **Nothing here defines a menu.** That is the point. A second definition of
//! the same menu drifts from the first, and the one that drifts is always the
//! one nobody is looking at — so the native side holds ids and labels it was
//! given, and calls `runCommand(id)` back into the webview. The only thing this
//! decides is presentation, and only where the platform differs.
//!
//! [`menuModel()`]: ../../webapp/editor.selection.js

use serde::Deserialize;

/// One top-level menu: File, Edit, View…
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Menu {
    pub id: String,
    pub label: String,
    pub items: Vec<Node>,
}

/// An entry within a menu.
///
/// `kind` is the tag the webview emits, so this deserialises the model as it
/// arrives rather than asking the JavaScript side to match a Rust shape.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Node {
    Separator,
    Item {
        id: String,
        label: String,
        #[serde(default)]
        accelerator: Option<String>,
        #[serde(default = "yes")]
        enabled: bool,
        #[serde(default)]
        checked: Option<bool>,
    },
    Submenu {
        id: String,
        label: String,
        items: Vec<Node>,
    },
}

fn yes() -> bool {
    true
}

/// Parse the JSON `menuModel()` returns.
pub fn parse(json: &str) -> Result<Vec<Menu>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Every command id in a menu tree, depth-first.
///
/// The shell registers one handler per id and forwards it to `runCommand`. An
/// id here that the webview does not know is a menu entry that throws when
/// clicked, so the shell checks this against `listCommands()` at startup rather
/// than discovering it from a user.
pub fn command_ids(menus: &[Menu]) -> Vec<String> {
    fn walk(items: &[Node], out: &mut Vec<String>) {
        for item in items {
            match item {
                Node::Separator => {}
                Node::Item { id, .. } => out.push(id.clone()),
                Node::Submenu { id, items, .. } => {
                    out.push(id.clone());
                    walk(items, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        walk(&menu.items, &mut out);
    }
    out
}

/// Commands whose native accelerator must be released while a cell is being
/// edited (`TAURI-012`).
///
/// **A native menu accelerator is consumed before the webview sees the key.**
/// That is the whole difference between this shell and the browser build, and
/// it makes the desktop *worse* rather than neutral: in a browser, `Cmd+T`
/// mid-formula reaches the editor's own handler and cycles the reference's
/// anchors. Here the menu ate it first and opened a modal over a half-typed
/// formula.
///
/// Microsoft's own Mac table gives `Cmd+T` **both** meanings — "cycle
/// absolute/relative references" and "create Table" — disambiguated by edit
/// mode. We took the Table half and none of the disambiguation.
///
/// The shell cannot know what a keystroke means; only the editor knows whether
/// a cell is open. So the editor says, and the shell does the one thing it can:
/// while an edit is open it releases the colliding accelerators, and the
/// keystroke reaches the webview exactly as it would in a browser.
///
/// **The set is deliberately small.** Releasing the whole menu during an edit
/// would be a different bug — Save is the shortcut people press *because* they
/// have been typing, and taking it away mid-edit is the moment it matters most.
/// Only chords Excel itself overloads are listed.
pub fn releases_during_edit(id: &str) -> bool {
    matches!(
        id,
        // `Cmd+T` / `Ctrl+T`: create Table, against Excel's anchor cycle.
        "insert.table"
            // `Shift+Cmd+T` on a Mac, so the same physical key with a shift.
            | "formula.autosum"
    )
}

/// Translate the shortcut the HTML menu displays into a native accelerator.
///
/// The editor writes Windows-style labels — `Ctrl+S`, `Ctrl+Shift+L` — because
/// that is what its own key handler binds and what most of its users type. A
/// native menu must show the platform's own: `⌘S` on macOS, `Ctrl+S` elsewhere.
///
/// `CmdOrCtrl` rather than branching on the host, because that is the token a
/// platform menu builder already resolves per-platform, and resolving it here
/// would mean this function had to know which machine it was running on to
/// produce a string that is then interpreted by something that also knows.
///
/// Returns `None` for a shortcut with no modifier and no recognised key, so a
/// decorative label never becomes a real binding.
pub fn accelerator(label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    // `Ctrl++` means Ctrl and the plus key, and splitting on `+` eats it. Named
    // before the split rather than inferred from an empty segment afterwards,
    // because `Ctrl+` produces an empty segment too and means nothing at all —
    // treating them alike turns a malformed label into a live binding.
    let normalised = if label.ends_with("++") {
        format!("{}Plus", &label[..label.len() - 1])
    } else {
        label.to_owned()
    };
    let mut parts: Vec<String> = Vec::new();
    let mut key = None;
    for raw in normalised.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        match part.to_ascii_lowercase().as_str() {
            // **`Ctrl` folds; `Control` does not** (`TAURI-012`).
            //
            // `CmdOrCtrl` is right when the platforms differ only in which
            // modifier they *name* — which is the ordinary case, and why the
            // editor's Windows-style labels can be written once. It is wrong
            // when they genuinely differ: Excel's insert-date is `Control+;` on
            // a Mac and `Ctrl+;` on Windows, the *same* physical key, which
            // folding would turn into `⌘;` and lose. Insert-time really is
            // `Cmd+;`. So a label that says `Control` is taken at its word.
            "control" => parts.push("Ctrl".to_owned()),
            "ctrl" | "cmd" | "command" => parts.push("CmdOrCtrl".to_owned()),
            "shift" => parts.push("Shift".to_owned()),
            "alt" | "option" => parts.push("Alt".to_owned()),
            other => {
                // The last non-modifier wins; a label with two keys is
                // malformed and the trailing one is what a reader sees.
                key = Some(match other {
                    "esc" => "Escape".to_owned(),
                    "del" => "Delete".to_owned(),
                    "ins" => "Insert".to_owned(),
                    "pgup" => "PageUp".to_owned(),
                    "pgdn" | "pgdown" => "PageDown".to_owned(),
                    _ => part.to_owned(),
                });
            }
        }
    }
    let key = key?;
    parts.push(key);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"[
      {"id":"file","label":"File","items":[
        {"kind":"item","id":"file.new","label":"New","accelerator":null,"enabled":true},
        {"kind":"separator"},
        {"kind":"submenu","id":"file.download","label":"Download","items":[
          {"kind":"item","id":"file.download.xlsx","label":"Excel","enabled":true}
        ]},
        {"kind":"item","id":"file.save","label":"Save","accelerator":"Ctrl+S","enabled":false}
      ]}
    ]"#;

    #[test]
    fn the_model_survives_the_crossing() {
        let menus = parse(MODEL).expect("parses");
        assert_eq!(menus.len(), 1);
        assert_eq!(menus[0].label, "File");
        // Four entries including the separator: a native menu without them is a
        // wall of verbs, so dropping it here would be a silent presentation bug.
        assert_eq!(menus[0].items.len(), 4);
        assert!(matches!(menus[0].items[1], Node::Separator));
    }

    #[test]
    fn a_disabled_item_stays_disabled() {
        let menus = parse(MODEL).unwrap();
        let Node::Item { enabled, label, .. } = &menus[0].items[3] else {
            panic!("expected an item");
        };
        assert_eq!(label, "Save");
        // The webview decides what is available — read-only mode disables half
        // the menu. A native bar that ignored this would offer edits the
        // document refuses, and the failure would surface as nothing happening.
        assert!(!enabled);
    }

    #[test]
    fn every_id_is_collected_including_submenus() {
        let ids = command_ids(&parse(MODEL).unwrap());
        assert_eq!(
            ids,
            vec![
                "file.new",
                "file.download",
                "file.download.xlsx",
                "file.save"
            ]
        );
    }

    /// **A native accelerator is consumed before the webview sees the key**
    /// (`TAURI-012`).
    ///
    /// That is the whole difference between the desktop shell and the browser
    /// build, and it makes the desktop *worse* rather than neutral: in a
    /// browser, `Cmd+T` mid-formula reaches the editor's own handler and cycles
    /// the reference's anchors. Here the menu eats it first and opens a modal
    /// over a half-typed formula.
    ///
    /// Microsoft's own Mac table gives `Cmd+T` **both** meanings — "cycle
    /// absolute/relative references" and "create Table" — disambiguated by edit
    /// mode. We took the Table half and none of the disambiguation.
    ///
    /// The shell cannot know what the keystroke means, so it does the one thing
    /// it can: while a cell is being edited it releases the accelerators that
    /// collide, and the webview gets the key it would have got in a browser.
    #[test]
    fn an_accelerator_that_collides_with_editing_is_released_while_editing() {
        // Excel's own overload, and the one that was reported.
        assert!(releases_during_edit("insert.table"));
        // AutoSum is `Shift+Cmd+T` on a Mac, so it collides with the same key.
        assert!(releases_during_edit("formula.autosum"));
        // F4 cycles anchors in Excel and is not a menu accelerator here, but
        // the anchor cycle is the behaviour being protected, so a menu item
        // that ever takes F4 must be in this set rather than silently shadow it.
        assert!(releases_during_edit("insert.table"));

        // Everything a person still expects to work mid-edit stays bound.
        // Releasing the whole menu during an edit would be a different bug:
        // Save is the one people press *because* they have been typing.
        assert!(!releases_during_edit("file.save"));
        assert!(!releases_during_edit("file.open"));
        assert!(!releases_during_edit("edit.undo"));
    }

    /// **Folding `Cmd` into `Ctrl` is not sufficient, and the chords prove it.**
    ///
    /// `CmdOrCtrl` is right when the two platforms differ only in which
    /// modifier they name. They do not always: Excel's insert-date is
    /// `Control+;` on a Mac and `Ctrl+;` on Windows — the *same* physical
    /// modifier, which `CmdOrCtrl` would wrongly turn into `⌘;` — while
    /// insert-time really is `Cmd+;`. AutoSum is `Shift+Cmd+T` against
    /// Windows' `Alt+=`, which is not the same chord at all.
    #[test]
    fn a_chord_that_differs_by_platform_is_not_folded() {
        // `Control+;` names the control key on both platforms. On macOS that is
        // `Ctrl`, not `Cmd`, so folding it loses Excel's actual binding.
        assert_eq!(accelerator("Control+;").as_deref(), Some("Ctrl+;"));
        // And the ordinary case still folds, because that is what it is for.
        assert_eq!(accelerator("Ctrl+S").as_deref(), Some("CmdOrCtrl+S"));
    }

    #[test]
    fn shortcuts_become_platform_accelerators() {
        assert_eq!(accelerator("Ctrl+S").as_deref(), Some("CmdOrCtrl+S"));
        assert_eq!(
            accelerator("Ctrl+Shift+L").as_deref(),
            Some("CmdOrCtrl+Shift+L")
        );
        assert_eq!(accelerator("F2").as_deref(), Some("F2"));
        assert_eq!(accelerator("Alt+=").as_deref(), Some("Alt+="));
        // `Ctrl++` splits into empty halves on `+`; the key *is* the `+`.
        assert_eq!(accelerator("Ctrl++").as_deref(), Some("CmdOrCtrl+Plus"));
        // But `Ctrl+` is a malformed label, not Ctrl-and-plus. It produces an
        // empty segment exactly like `Ctrl++` does, and an earlier version read
        // both the same way — which would have registered a live accelerator
        // for a typo and swallowed the key in the process.
        assert_eq!(accelerator("Ctrl+"), None);
        assert_eq!(accelerator(""), None);
        // A modifier with no key is not a shortcut.
        assert_eq!(accelerator("Ctrl"), None);
    }

    #[test]
    fn named_keys_get_their_platform_spelling() {
        assert_eq!(
            accelerator("Ctrl+PgDn").as_deref(),
            Some("CmdOrCtrl+PageDown")
        );
        assert_eq!(accelerator("Esc").as_deref(), Some("Escape"));
        assert_eq!(accelerator("Ctrl+Del").as_deref(), Some("CmdOrCtrl+Delete"));
    }
}
