//! Closing the window when there is unsaved work (`TAURI-011`).
//!
//! Before this, the native close button and the menu's Quit discarded unsaved
//! work **in silence**. The editor's `beforeunload` is a web affordance and
//! neither route goes through it, so the only warning the product had was one
//! the desktop shell could not reach.
//!
//! # Why the answer arrives separately
//!
//! `WindowEvent::CloseRequested` is a synchronous callback on the event loop.
//! The question — *you have unsaved work, really close?* — is answered in the
//! webview, asynchronously, because the editor is the only thing that knows
//! whether the document is dirty and the only thing with a dialog that looks
//! like the rest of the application.
//!
//! So the first request is always refused, the webview is asked, and the answer
//! comes back as a command that sets a latch and closes again. The second
//! request sees the latch and passes through. That is the whole mechanism, and
//! this module holds the two decisions in it that can be checked without a
//! window.
//!
//! # What is *not* covered, said plainly
//!
//! macOS `Cmd+Q` terminates the application rather than closing a window, and
//! arrives as `RunEvent::ExitRequested`, not here. A user who quits that way is
//! still not asked. That is a second route and a separate row; claiming this
//! module closes it would be worse than leaving it open.

/// Whether a close request should be refused so the user can be asked.
///
/// `agreed` is the latch set by the webview's answer. The **only** reason to
/// let a request through is that somebody already said yes: a close that is
/// allowed for any other reason is a close nobody was asked about, which is the
/// defect this exists to remove.
#[must_use]
pub const fn should_prevent(agreed: bool) -> bool {
    !agreed
}

/// The script asked in the webview when a close is requested.
///
/// Held here rather than inline so its contract is checkable: it must ask the
/// editor whether the document is dirty, it must agree immediately when it is
/// not, and every path through it must end in `agree_to_close` or in a
/// deliberate cancellation. A script that can end without doing either traps
/// the user in a window that will not shut, which is a worse failure than the
/// one being fixed.
pub const CONFIRM_CLOSE: &str = r#"(async () => {
  const e = window.opencalcEditor;
  const invoke = window.__TAURI__.core.invoke;
  // No editor, or no way to ask: close rather than trap the user in a window
  // that will not shut. A shell that cannot be closed is worse than one that
  // closes without asking.
  if (!e || !e.isDirty || !e.isDirty()) {
    return await invoke("agree_to_close");
  }
  const ok = e.confirmModal
    ? await e.confirmModal(
        "Close without saving?",
        "This workbook has changes that have not been saved. Closing discards them, and undo will not bring them back.",
        "Discard and close",
      )
    : true;
  if (ok) await invoke("agree_to_close");
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The latch is the only thing that lets a close through.
    #[test]
    fn a_close_is_refused_until_somebody_has_agreed_to_it() {
        assert!(
            should_prevent(false),
            "a close nobody has agreed to must be refused, or the question is never asked"
        );
        assert!(
            !should_prevent(true),
            "once agreed, the close must go through — otherwise the window cannot be shut at all"
        );
    }

    /// The script asks the editor rather than assuming.
    #[test]
    fn the_question_is_asked_of_the_editor_and_answered_back_to_the_shell() {
        assert!(
            CONFIRM_CLOSE.contains("isDirty"),
            "the script must ask whether there is unsaved work; without it every close asks, \
             and a dialog on every close is one people learn to dismiss without reading"
        );
        assert!(
            CONFIRM_CLOSE.contains("agree_to_close"),
            "the script must be able to answer, or the window never closes"
        );
        assert!(
            CONFIRM_CLOSE.contains("confirmModal"),
            "the question must use the editor's own dialog, not a native one that looks foreign"
        );
    }

    /// A document with nothing unsaved closes without a dialog.
    ///
    /// Asserted on the script's shape because the behaviour needs a window: the
    /// clean path must reach `agree_to_close` *before* any `confirmModal` call,
    /// so an ordinary close costs one round trip and no question.
    #[test]
    fn a_clean_document_agrees_without_asking() {
        let agree = CONFIRM_CLOSE
            .find("agree_to_close")
            .expect("the script must answer");
        let modal = CONFIRM_CLOSE
            .find("confirmModal")
            .expect("the script must be able to ask");
        assert!(
            agree < modal,
            "the clean-document path must agree before the dialog is reached, or every close asks"
        );
    }
}
