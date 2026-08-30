//! A file the operating system hands to this application.
//!
//! `TAURI-010`. Declaring `bundle.fileAssociations` is the half everybody
//! remembers; it is also the half that is *worse than nothing* on its own,
//! because a registered association that the application cannot honour turns a
//! double-click into a blank window — the user's file is now owned by a program
//! that appears to lose it. So the rule this module exists to keep is: **the
//! opening works first, and the association is declared against it.**
//!
//! Three ways in, and they are not interchangeable:
//!
//! * **Windows and Linux** put the path in `argv`. A `.desktop` file's `Exec`
//!   line may spell it `%F` (a path) or `%U` (a `file://` URL), and which one a
//!   given bundler writes is not this code's to choose, so both are accepted.
//! * **macOS never uses `argv` for this.** Finder sends
//!   `application:openURLs:`, which Tauri surfaces as `RunEvent::Opened`. It is
//!   the *only* route on that platform, and it can arrive **before** there is a
//!   window to open the file into — so a path is queued rather than acted on,
//!   and [`PendingOpen`] is what holds it until the webview says it is ready.
//! * **A second activation of a running application** — double-clicking another
//!   file while the window is open — arrives the same way and must not be
//!   dropped. That is why readiness is a *flag inside the same lock* as the
//!   queue rather than an event: a path that arrives in the instant the webview
//!   announces itself must be collected by exactly one of the two paths, never
//!   by neither.
//!
//! **Nothing here reads a file.** The shell's invariant is that a path may
//! cross *into* the process only from the platform (`main.rs`), and this module
//! is the part that decides whether a path the platform offered names something
//! this engine can open at all. That question has one authority —
//! [`casual_calc_sdk::SessionFormat::for_extension`] — and it is asked rather
//! than answered from a list.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Every extension worth asking the SDK about.
///
/// Candidates, **not** an answer: which of them this build actually opens is
/// decided by [`casual_calc_sdk::SessionFormat::for_extension`], never by
/// reading this list. That is the same construction
/// `casual_calc_wasm::io::openable_extensions` uses for the browser's file
/// picker, and it is deliberate that both ask the same authority — a format the
/// SDK learns appears in the panel *and* in the association set on the day it
/// does, rather than in whichever of the two somebody remembered.
///
/// The two candidate lists are separate because the crates are: `desktop/` is
/// its own cargo workspace (`ADR-023`) and does not depend on
/// `casual-calc-wasm`. `associations_match_what_the_engine_opens` is the gate
/// that keeps this one honest against the file it actually governs.
const CANDIDATE_EXTENSIONS: [&str; 7] = ["xlsx", "xlsm", "ods", "csv", "tsv", "tab", "psv"];

/// The extensions this build can open, lower-case and without a leading dot.
///
/// What `bundle.fileAssociations` must declare, and no more. An association for
/// an extension the engine refuses is the defect `TAURI-010` names; an
/// extension the engine reads and the bundle omits is a file the platform hands
/// to somebody else.
#[must_use]
pub fn openable_extensions() -> Vec<String> {
    CANDIDATE_EXTENSIONS
        .iter()
        .filter(|ext| casual_calc_sdk::SessionFormat::for_extension(ext).is_some())
        .map(|ext| (*ext).to_owned())
        .collect()
}

/// Does `path` name something this engine will open?
///
/// Asked of the SDK, so `.xlsm` became answerable here the day `IO-08` taught
/// the engine to read it and not one commit later. A path that fails this is
/// ignored in silence: an argument this application does not recognise is not
/// an error worth a dialog, and the alternative — opening it anyway — is how a
/// spreadsheet is asked to parse a binary.
#[must_use]
pub fn opens_here(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| casual_calc_sdk::SessionFormat::for_extension(ext).is_some())
}

/// The file named on the command line, if the command line names one.
///
/// Windows and Linux only in practice, but not gated on the platform: gating it
/// would make the one code path that `cargo test` can exercise the one that
/// does not run on the machine the tests run on.
///
/// The rules, each because of something a real launcher does:
///
/// * `argv[0]` is skipped — it is this binary, and on Windows it is an absolute
///   path that would otherwise have to be excluded by its extension.
/// * Anything beginning with `-` is skipped. macOS's Finder appends
///   `-psn_0_12345` when it launches a bundle, and a `--flag` is not a file.
/// * A `file://` argument is decoded, because a `.desktop` file written with
///   `%U` delivers one.
/// * The **first** argument that survives and names a format the engine opens
///   wins; the rest are ignored, because this shell is one window showing one
///   workbook ([`docs/86`] §6.2) and there is nowhere to put a second file.
///
/// [`docs/86`]: ../../docs/86-DESKTOP-RELEASE-IDENTITY-SETTINGS-AND-UPDATES.md
#[must_use]
pub fn path_from_args<I: IntoIterator<Item = OsString>>(args: I) -> Option<PathBuf> {
    args.into_iter()
        .skip(1)
        .filter_map(|arg| {
            // **The UTF-8 question is asked of the flag and URL rules, not of
            // the path.** A filename on Linux is a byte string and is under no
            // obligation to be UTF-8; `arg.to_str()?` at the top of this closure
            // would have silently refused to open every such file, on the one
            // platform where they occur. A path that is not UTF-8 simply cannot
            // begin with `-` or `file:` — those are ASCII — so it falls through
            // to the branch that never needed the conversion.
            match arg.to_str() {
                Some(text) if text.starts_with('-') => None,
                // A URL that is not a local file is dropped, not passed through
                // as a literal path: `file://server/share/x.xlsx` ends in
                // `.xlsx` and would otherwise satisfy [`opens_here`] and be
                // handed to `std::fs::read` as a relative path beginning with
                // `file:`.
                Some(text) if text.starts_with("file:") => strip_file_url(text).map(PathBuf::from),
                _ => Some(PathBuf::from(arg)),
            }
        })
        .find(|path| opens_here(path))
}

/// `file:///Users/a/b.xlsx` as a path, or `None` for anything else.
///
/// Percent-decoded, because a URL is where `figures 2024.xlsx` becomes
/// `figures%202024.xlsx` and a shell that did not decode would report that no
/// such file exists — for the one class of filename users have most of.
///
/// Only `file://` with an empty or `localhost` authority: a `file://server/`
/// UNC-style URL names somebody else's machine, and this is not the code that
/// should decide to read from it.
fn strip_file_url(text: &str) -> Option<String> {
    let rest = text.strip_prefix("file://")?;
    let rest = rest
        .strip_prefix("localhost/")
        .map(|r| format!("/{r}"))
        .unwrap_or_else(|| rest.to_owned());
    if !rest.starts_with('/') {
        return None;
    }
    Some(percent_decode(&rest))
}

/// `%20` back to a space, and every other pair back to its byte.
///
/// A trailing `%` or a pair that is not hexadecimal is left alone rather than
/// dropped: it is far likelier to be a filename that genuinely contains a
/// percent sign than a truncated escape, and losing a character silently is how
/// a path stops naming the file the user clicked.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Lossy rather than refusing: a path that is not UTF-8 is still a path, and
    // the read that follows will say so in the platform's own words.
    String::from_utf8_lossy(&out).into_owned()
}

/// A file the platform handed over, waiting for a window to open it in.
///
/// The whole point is the second field. On macOS `RunEvent::Opened` can arrive
/// before the webview exists, and the naive shape — emit an event and hope
/// somebody is listening — drops exactly the case the feature is for: the first
/// launch, which is the only one most users ever see.
///
/// So readiness lives *here*, beside the queue and under the same lock, and it
/// is set by the webview collecting rather than by the shell guessing. Every
/// path is therefore delivered by exactly one of two routes and never by both:
/// the collection that the webview performs when it becomes ready, or the nudge
/// [`queue`](Self::queue) asks for when it already was.
#[derive(Debug, Default)]
pub struct PendingOpen {
    queued: Option<PathBuf>,
    webview_ready: bool,
}

impl PendingOpen {
    /// Hold `path` for the window.
    ///
    /// Returns `true` when the webview is already collecting, which means the
    /// caller must nudge it — a second activation of a running application, and
    /// the case that is dropped by every implementation that only handles the
    /// first launch.
    ///
    /// A second file replaces the first rather than queueing behind it: one
    /// window shows one workbook, and holding a file the window will never get
    /// to is worse than the user re-opening it.
    pub fn queue(&mut self, path: PathBuf) -> bool {
        self.queued = Some(path);
        self.webview_ready
    }

    /// The webview is ready and is collecting whatever is waiting.
    ///
    /// Marks readiness **whether or not there is anything queued**, because
    /// that is the fact the next [`queue`](Self::queue) needs: a launch with no
    /// file still has to leave the shell able to nudge a later one.
    pub fn take(&mut self) -> Option<PathBuf> {
        self.webview_ready = true;
        self.queued.take()
    }

    /// This window is showing a different document now, so nothing is waiting.
    ///
    /// Readiness survives: the webview did not go away.
    pub fn clear(&mut self) {
        self.queued = None;
    }

    /// Has the webview announced itself?
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.webview_ready
    }

    /// What is waiting, without taking it.
    #[must_use]
    pub fn queued(&self) -> Option<&Path> {
        self.queued.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(raw: &[&str]) -> Vec<OsString> {
        raw.iter().map(OsString::from).collect()
    }

    /// The Windows and Linux route: a path in `argv`, and a window that opens
    /// it. Everything else on the command line is noise.
    #[test]
    fn the_path_comes_out_of_argv() {
        assert_eq!(
            path_from_args(args(&["opencalc-desktop", "/tmp/figures.xlsx"])),
            Some(PathBuf::from("/tmp/figures.xlsx")),
        );
        // argv[0] is not a document, even when it is the only argument.
        assert_eq!(path_from_args(args(&["opencalc-desktop"])), None);
        // Finder's serial-number argument, which is on every macOS launch.
        assert_eq!(
            path_from_args(args(&["opencalc-desktop", "-psn_0_774242"])),
            None,
        );
        // A flag before the file must not swallow it — and a flag that *ends*
        // in a spreadsheet extension must not be mistaken for one. Without the
        // leading-`-` rule this returns a "path" called `--open=/tmp/a.xlsx`,
        // because `Path::extension` is perfectly happy with it.
        assert_eq!(
            path_from_args(args(&["opencalc-desktop", "--verbose", "/tmp/a.csv"])),
            Some(PathBuf::from("/tmp/a.csv")),
        );
        assert_eq!(
            path_from_args(args(&["opencalc-desktop", "--open=/tmp/a.xlsx"])),
            None,
        );
    }

    /// A filename on Linux is bytes, not text.
    ///
    /// `OsStr::to_str` returns `None` for one, and asking that question of
    /// *every* argument — rather than only of the flag and URL rules that
    /// genuinely need ASCII — silently refuses to open a whole class of real
    /// files on the one platform where they exist.
    #[cfg(unix)]
    #[test]
    fn a_filename_that_is_not_utf8_still_opens() {
        use std::os::unix::ffi::OsStringExt;
        // 0xFF is not valid UTF-8 anywhere in a sequence.
        let raw = OsString::from_vec(b"/tmp/caf\xff.xlsx".to_vec());
        assert!(raw.to_str().is_none(), "the premise: this is not UTF-8");
        assert_eq!(
            path_from_args(vec![OsString::from("opencalc-desktop"), raw.clone()]),
            Some(PathBuf::from(raw)),
        );
    }

    /// `.xlsm` is the one that proves the list is not a list.
    ///
    /// It became openable in `IO-08`, after this shell was written. Nothing in
    /// `desktop/` was edited to make this pass — the answer comes from
    /// `SessionFormat::for_extension`, which is the whole reason the question
    /// is asked rather than looked up.
    #[test]
    fn the_engines_answer_decides_which_files_open() {
        for ext in ["xlsx", "xlsm", "ods", "csv", "tsv", "tab", "psv"] {
            let path = PathBuf::from(format!("/tmp/figures.{ext}"));
            assert!(opens_here(&path), ".{ext} is a format this engine reads");
        }
        // And the ones it does not, which must not be associated either: a
        // double-click on a `.numbers` that landed here would be a package
        // opened as a spreadsheet.
        for ext in ["numbers", "xls", "pdf", "exe", "txt"] {
            let path = PathBuf::from(format!("/tmp/figures.{ext}"));
            assert!(!opens_here(&path), ".{ext} is not");
        }
        // No extension at all is not a spreadsheet either.
        assert!(!opens_here(Path::new("/tmp/figures")));
    }

    /// A `.desktop` file written with `%U` hands over a URL, not a path.
    #[test]
    fn a_file_url_from_a_desktop_entry_is_a_path() {
        assert_eq!(
            path_from_args(args(&["app", "file:///home/a/figures.xlsx"])),
            Some(PathBuf::from("/home/a/figures.xlsx")),
        );
        // The case that breaks a naive `strip_prefix`: the space users
        // actually put in filenames arrives percent-encoded.
        assert_eq!(
            path_from_args(args(&["app", "file:///home/a/Q3%20figures.csv"])),
            Some(PathBuf::from("/home/a/Q3 figures.csv")),
        );
        assert_eq!(
            path_from_args(args(&["app", "file://localhost/home/a/x.tsv"])),
            Some(PathBuf::from("/home/a/x.tsv")),
        );
        // Somebody else's machine. Not this code's decision to read from.
        assert_eq!(
            path_from_args(args(&["app", "file://server/share/x.xlsx"])),
            None
        );
        // A literal percent in a filename survives; a truncated escape is not
        // an escape.
        assert_eq!(percent_decode("/a/100%.csv"), "/a/100%.csv");
        assert_eq!(percent_decode("/a/b%zz.csv"), "/a/b%zz.csv");
    }

    /// The macOS ordering that this type exists for.
    ///
    /// `RunEvent::Opened` before the webview is ready, which is the first
    /// launch — the case a bare event would drop, and the case every user sees
    /// first.
    #[test]
    fn a_file_that_arrives_before_the_window_is_held_until_it_can_be_opened() {
        let mut pending = PendingOpen::default();
        let nudge = pending.queue(PathBuf::from("/tmp/early.xlsx"));
        assert!(
            !nudge,
            "nothing to nudge yet — evaluating into a webview that does not \
             exist is how this file gets lost"
        );
        assert_eq!(pending.queued(), Some(Path::new("/tmp/early.xlsx")));
        assert!(!pending.is_ready());

        // The webview announces itself and collects.
        assert_eq!(pending.take(), Some(PathBuf::from("/tmp/early.xlsx")));
        assert!(pending.is_ready());
        assert_eq!(pending.take(), None, "taken once, not handed out twice");
    }

    /// The second activation: another file double-clicked while the window is
    /// open. It must be nudged, because nobody is going to come and ask.
    #[test]
    fn a_file_that_arrives_after_the_window_is_nudged_rather_than_dropped() {
        let mut pending = PendingOpen::default();
        // A launch with no file still makes the webview ready.
        assert_eq!(pending.take(), None);
        assert!(pending.is_ready());

        let nudge = pending.queue(PathBuf::from("/tmp/second.csv"));
        assert!(nudge, "the running window has to be told");
        assert_eq!(pending.take(), Some(PathBuf::from("/tmp/second.csv")));

        // And a third, because "works once" is the shape of this bug.
        assert!(pending.queue(PathBuf::from("/tmp/third.ods")));
        assert_eq!(pending.take(), Some(PathBuf::from("/tmp/third.ods")));
    }

    /// One window, one workbook: the later file wins rather than queueing.
    #[test]
    fn two_files_before_the_window_leave_the_last_one() {
        let mut pending = PendingOpen::default();
        pending.queue(PathBuf::from("/tmp/first.xlsx"));
        pending.queue(PathBuf::from("/tmp/second.xlsx"));
        assert_eq!(pending.take(), Some(PathBuf::from("/tmp/second.xlsx")));
    }

    /// **The gate this row exists for.**
    ///
    /// `bundle.fileAssociations` and the engine's own answer must be the same
    /// set. An extension declared here that the engine refuses is a
    /// double-click that opens a blank window — the defect `TAURI-010` is
    /// named after — and one the engine reads and the bundle omits is a file
    /// the platform quietly hands to somebody else.
    ///
    /// Read from the real `tauri.conf.json`, because that is the file the
    /// installer is built from; a constant in this crate would be a third copy
    /// of the list and the one that agrees with nothing.
    #[test]
    fn associations_match_what_the_engine_opens() {
        let config = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let text = std::fs::read_to_string(&config).expect("tauri.conf.json");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let associations = json["bundle"]["fileAssociations"]
            .as_array()
            .expect("bundle.fileAssociations is declared — without it a double-click does nothing");

        let mut declared: Vec<String> = associations
            .iter()
            .flat_map(|entry| {
                entry["ext"]
                    .as_array()
                    .expect("every association lists its extensions")
                    .iter()
                    .map(|ext| {
                        ext.as_str()
                            .expect("an extension is a string")
                            .trim_start_matches('.')
                            .to_ascii_lowercase()
                    })
            })
            .collect();
        let before = declared.len();
        declared.sort();
        declared.dedup();
        assert_eq!(before, declared.len(), "an extension is declared twice");

        let mut engine = openable_extensions();
        engine.sort();
        assert_eq!(
            declared, engine,
            "bundle.fileAssociations and SessionFormat::for_extension disagree"
        );

        // The other half of the promise, and the reason the row exists: an
        // association is only honest if the shell can act on it. Every declared
        // extension has to survive the same check the argv and `Opened` routes
        // apply, or the association leads to a window that opens nothing.
        for ext in &declared {
            assert!(
                opens_here(&PathBuf::from(format!("/tmp/x.{ext}"))),
                ".{ext} is associated but the shell would ignore the path"
            );
        }
    }

    /// Every association carries a role and a rank, and none of them claims to
    /// own a format somebody else defined.
    ///
    /// `LSHandlerRank: Owner` on `.xlsx` would tell macOS this application is
    /// the primary creator of Excel workbooks. It is not, and a launcher that
    /// believes it starts handing Excel's files here ahead of Excel.
    #[test]
    fn no_association_claims_to_own_a_format_it_did_not_define() {
        let config = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let text = std::fs::read_to_string(&config).expect("tauri.conf.json");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let associations = json["bundle"]["fileAssociations"]
            .as_array()
            .expect("bundle.fileAssociations is declared");
        for entry in associations {
            let rank = entry["rank"]
                .as_str()
                .expect("every association ranks itself");
            assert_ne!(rank, "Owner", "this engine defined none of these formats");
            assert!(
                ["Default", "Alternate"].contains(&rank),
                "unexpected LSHandlerRank {rank}"
            );
            assert_eq!(
                entry["role"].as_str(),
                Some("Editor"),
                "a spreadsheet that can save is an editor, not a viewer"
            );
            assert!(
                entry["name"].as_str().is_some_and(|n| !n.is_empty()),
                "the name is what Windows Explorer and Finder show in the Kind column"
            );
            // `text/plain` is the one mime type that must never appear here.
            // `tauri_utils::config::mime_type_to_uti` maps it to
            // `public.plain-text`, which goes into `LSItemContentTypes` — and
            // an application that declares `public.plain-text` has told macOS
            // it opens *every* text file, source code included. `.psv` has no
            // registered type, so it carries none: the association is then by
            // extension on macOS and Windows, and absent on Linux, which is
            // narrower than the truth rather than wider.
            assert_ne!(
                entry["mimeType"].as_str(),
                Some("text/plain"),
                "text/plain would claim every text file on this machine"
            );
        }
    }
}
