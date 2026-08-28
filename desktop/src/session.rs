//! What the shell knows about the document in its window.
//!
//! Two things, and both come from the webview: which file is open and whether
//! it has unsaved changes (the title bar), and which capabilities this mode
//! grants (whether a native Open is allowed at all).
//!
//! **The capability check is not skipped because the dialog is native.** The
//! editor resolves modes into declared capabilities — `canOpen`, `canSaveAs`,
//! `ownsFile` and the rest — and `applyCommandRules()` hides the commands a
//! mode forbids. A native menu built from `menuModel()` inherits that for free,
//! because the model is read from the live DOM. What it does *not* cover is the
//! Tauri command itself: `native_open` is reachable from any script in the
//! webview, so the permission has to exist on this side of the bridge too.
//!
//! It cannot be *asked for* — a command handler cannot call synchronously into
//! JavaScript — so the webview **tells** the shell, at boot and whenever
//! `setCapabilities` changes anything. Until it does, everything is refused: a
//! shell that assumed permission until told otherwise would grant it for the
//! whole of boot, which is exactly the window a page that is not our editor
//! would use.

use serde::Deserialize;

use crate::dialog;
use crate::title;

/// What the editor's `getCapabilities()` says, as far as this side cares.
///
/// A subset on purpose: `canPrint`, `chrome` and `mode` decide nothing here,
/// and serde ignores what it is not asked for — so the webview sends the whole
/// resolved set and this does not have to be edited when a seventh axis is
/// added.
///
/// Every field defaults to `false`, which is the *restrictive* answer. That is
/// the opposite of the editor's own default (all-`true` for standalone) and
/// deliberately so: there the default describes a user's own page, here it
/// describes a report that has not arrived.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    pub can_open: bool,
    pub can_save_as: bool,
    pub owns_file: bool,
    pub read_only: bool,
}

impl Capabilities {
    /// May the user replace the document in this window?
    ///
    /// `ownsFile` forces it off and cannot be overridden back on — the same
    /// rule `resolveCapabilities()` applies in the editor, restated here rather
    /// than assumed, because this side receives a *report* and a report can be
    /// stale, partial, or sent by something that is not the editor.
    pub fn may_open(&self) -> bool {
        self.can_open && !self.owns_file
    }

    /// May the user write a copy out through the platform's save panel?
    pub fn may_save_as(&self) -> bool {
        self.can_save_as
    }
}

/// The document this window is showing, and what the mode allows.
#[derive(Debug, Default)]
pub struct Session {
    document: Option<String>,
    dirty: bool,
    capabilities: Capabilities,
}

impl Session {
    /// The window title for the current document and dirty state.
    pub fn title(&self) -> String {
        title::window_title(self.document.as_deref(), self.dirty)
    }

    /// The document changed, or its dirty state did.
    pub fn set_document(&mut self, name: Option<String>, dirty: bool) {
        self.document = name.filter(|n| !n.trim().is_empty());
        self.dirty = dirty;
    }

    /// The name of the open document, if there is one.
    pub fn document(&self) -> Option<&str> {
        self.document.as_deref()
    }

    /// The webview's report of what this mode grants.
    pub fn set_capabilities(&mut self, capabilities: Capabilities) {
        self.capabilities = capabilities;
    }

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    /// Refuse a native Open the mode does not allow.
    ///
    /// The message names the capability, because the two ways to reach it —
    /// a mode that never had it, and a report that has not arrived yet — look
    /// identical from the webview and are debugged differently.
    pub fn guard_open(&self) -> Result<(), String> {
        if self.capabilities.may_open() {
            Ok(())
        } else {
            Err("this window may not open another document (canOpen is off)".to_owned())
        }
    }

    /// Refuse a native Save the mode does not allow.
    pub fn guard_save(&self) -> Result<(), String> {
        if self.capabilities.may_save_as() {
            Ok(())
        } else {
            Err("this window may not write a copy out (canSaveAs is off)".to_owned())
        }
    }

    /// What the save panel should propose for `ext`.
    pub fn suggested_save_name(&self, ext: &str) -> String {
        dialog::suggested_file_name(self.document.as_deref(), ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the editor actually sends: the whole resolved set, six axes plus
    /// the mode name.
    const DESKTOP_REPORT: &str = r#"{
        "canOpen": true, "canSaveAs": true, "canPrint": true,
        "ownsFile": false, "chrome": "native", "readOnly": false,
        "mode": "desktop"
    }"#;

    #[test]
    fn a_shell_that_has_not_been_told_refuses_everything() {
        // The window exists before the editor boots. A native Open in that gap
        // would be a dialog nothing asked for, and a shell that assumed
        // permission until contradicted would grant it for the whole of boot.
        let session = Session::default();
        let refused = session.guard_open().expect_err("nothing has been reported");
        assert!(refused.contains("canOpen"), "names the axis: {refused}");
        assert!(session.guard_save().is_err());
    }

    #[test]
    fn the_editors_own_report_parses_and_grants() {
        let caps: Capabilities = serde_json::from_str(DESKTOP_REPORT).expect("the report parses");
        // Axes this side does not use must not make the report unreadable —
        // `canPrint`, `chrome` and `mode` are all in it.
        assert!(caps.may_open() && caps.may_save_as());
        let mut session = Session::default();
        session.set_capabilities(caps);
        assert!(session.guard_open().is_ok());
        assert!(session.guard_save().is_ok());
    }

    #[test]
    fn a_host_that_owns_the_file_cannot_be_talked_into_an_open() {
        // The editor's rule, restated on this side because this side receives a
        // report rather than computing one. A `{canOpen: true, ownsFile: true}`
        // report is either stale or forged; either way the answer is no.
        let caps: Capabilities =
            serde_json::from_str(r#"{"canOpen": true, "ownsFile": true, "canSaveAs": true}"#)
                .unwrap();
        let mut session = Session::default();
        session.set_capabilities(caps);
        assert!(session.guard_open().is_err(), "ownsFile wins");
        // `canSaveAs` is deliberately not forced the same way: "download a
        // copy" is a permission a host grants per user, and the editor lets a
        // host turn it back on.
        assert!(session.guard_save().is_ok());
    }

    #[test]
    fn the_title_follows_the_document() {
        let mut session = Session::default();
        assert_eq!(session.title(), "Untitled — OpenCalc");
        session.set_document(Some("figures.xlsx".to_owned()), false);
        assert_eq!(session.title(), "figures.xlsx — OpenCalc");
        session.set_document(Some("figures.xlsx".to_owned()), true);
        assert_eq!(session.title(), "• figures.xlsx — OpenCalc");
        // A blank name is no name — the bridge sends one when a host clears
        // the document, and " — OpenCalc" reads as a bug.
        session.set_document(Some("   ".to_owned()), false);
        assert_eq!(session.title(), "Untitled — OpenCalc");
        assert_eq!(session.document(), None);
    }

    #[test]
    fn the_save_panel_proposes_the_open_documents_name() {
        let mut session = Session::default();
        session.set_document(Some("figures.xlsx".to_owned()), true);
        assert_eq!(session.suggested_save_name("csv"), "figures.csv");
        assert_eq!(session.suggested_save_name("xlsx"), "figures.xlsx");
    }
}
