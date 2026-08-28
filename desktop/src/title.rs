//! What the window's title bar says.
//!
//! A desktop application's title bar names the document, not the program:
//! `figures.xlsx — OpenCalc`, and an edited marker while there is unsaved work.
//! The editor already knows both halves — the name it opened and `isDirty()`
//! from `SEC-019` — so this only decides how they read.
//!
//! **The marker is a leading `•`, not the platform's own.** macOS has a real
//! document-edited indicator (the dot in the close button, `NSWindow
//! setDocumentEdited:`), and `tao` exposes it as
//! `WindowExtMacOS::set_is_document_edited`. Tauri 2.11 does not re-export it,
//! and reaching the `NSWindow` through `ns_window()` is a raw pointer and a
//! message send — which this crate forbids (`unsafe_code = "forbid"` in
//! `Cargo.toml`). A bullet is the honest fallback and is what Windows and most
//! Linux desktops show anyway.

/// The program's name, as it appears after the document.
pub const APP_NAME: &str = "OpenCalc";

/// What a window with no document says.
pub const UNTITLED: &str = "Untitled";

/// The unsaved-work marker.
pub const EDITED_MARKER: &str = "• ";

/// How much of a file name a title bar will carry.
///
/// A file name has no length limit worth relying on, and a title bar that is
/// wider than the screen pushes the window buttons out of reach on some
/// desktops. Truncated at a character boundary, never a byte one.
const MAX_NAME_CHARS: usize = 120;

/// `figures.xlsx — OpenCalc`, with a marker when there is unsaved work.
///
/// The separator is an em dash with spaces, which is the convention on every
/// desktop this ships to — and it is *not* a delimiter this parses back, so a
/// document called `Q3 — final.xlsx` keeps its own dash rather than being
/// escaped or split.
pub fn window_title(document: Option<&str>, dirty: bool) -> String {
    let name = document.map(clean).unwrap_or_default();
    let name = if name.is_empty() { UNTITLED } else { &name };
    let marker = if dirty { EDITED_MARKER } else { "" };
    format!("{marker}{name} — {APP_NAME}")
}

/// A file name reduced to something a title bar can show.
///
/// Control characters are legal in a POSIX file name and a newline in a window
/// title is at best ugly and at worst a way to push the program's own name out
/// of sight. They become spaces, runs of whitespace collapse, and the result is
/// trimmed — so `"  report\n\tQ3.xlsx "` reads as `report Q3.xlsx`.
fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut space = false;
    for ch in raw.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if ch.is_whitespace() {
            // Leading whitespace never opens a run, so no trailing trim is
            // needed beyond the one below.
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        if out.chars().count() == MAX_NAME_CHARS - 1 {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_comes_before_the_program() {
        assert_eq!(
            window_title(Some("figures.xlsx"), false),
            "figures.xlsx — OpenCalc"
        );
    }

    #[test]
    fn unsaved_work_is_marked() {
        assert_eq!(
            window_title(Some("figures.xlsx"), true),
            "• figures.xlsx — OpenCalc"
        );
    }

    #[test]
    fn a_window_with_no_document_is_untitled() {
        assert_eq!(window_title(None, false), "Untitled — OpenCalc");
        // An empty or blank name is the same situation as no name at all. It
        // arrives whenever a host calls the bridge before a file is open, and
        // " — OpenCalc" with nothing in front of it looks like a bug.
        assert_eq!(window_title(Some(""), false), "Untitled — OpenCalc");
        assert_eq!(window_title(Some("   "), true), "• Untitled — OpenCalc");
    }

    #[test]
    fn a_name_with_an_em_dash_survives_intact() {
        // The separator is presentation, not syntax: nothing parses the title
        // back, so a document whose own name contains the separator is carried
        // verbatim rather than escaped, split, or quoted.
        assert_eq!(
            window_title(Some("Q3 — final.xlsx"), true),
            "• Q3 — final.xlsx — OpenCalc"
        );
    }

    #[test]
    fn control_characters_cannot_break_the_title_apart() {
        // A newline is a legal character in a POSIX file name. In a title bar
        // it is a way to push "OpenCalc" onto a line nobody sees.
        assert_eq!(
            window_title(Some("bad\nname\t.csv"), false),
            "bad name .csv — OpenCalc"
        );
    }

    #[test]
    fn an_enormous_name_is_cut_at_a_character_boundary() {
        // Multibyte on purpose: cutting by bytes panics here rather than
        // truncating, and a panic inside a title update takes the window with
        // it.
        let long = "é".repeat(400) + ".xlsx";
        let title = window_title(Some(&long), false);
        assert_eq!(
            title.chars().count(),
            MAX_NAME_CHARS + " — OpenCalc".chars().count()
        );
        assert!(title.contains('…'), "the cut is visible: {title}");
        assert!(title.ends_with(" — OpenCalc"));
    }
}
