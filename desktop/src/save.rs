//! Writing the document back to the file it was opened from.
//!
//! [`docs/83`](../../docs/83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md) §2 states the
//! rule this module exists to keep: *a document has one save target; `Ctrl+S`
//! commits the document to that target and never creates a second document.*
//! Phase A (`SAVE-02`) is the `file` target — a path a platform panel returned.
//!
//! **The "bytes, never paths" invariant is restated, not relaxed.** `main.rs`
//! enforces it by shape: no command accepts a path and no command hands one
//! out. That still holds. What changes is that the shell now *remembers* one
//! path, and it only ever remembers a value a platform panel gave it. The
//! webview cannot name a destination; it can only say "the one the user already
//! chose".
//!
//! Everything here is a value in, a value out, so the part that decides whether
//! a user still has their file is provable by `cargo test` rather than by
//! launching a window and hoping.
//!
//! ## Why the write is a temporary file and a rename
//!
//! `std::fs::write` truncates first. A save that fails after the truncate and
//! before the last byte leaves the user with **neither** the old file nor the
//! new one, and the shapes that produce it are ordinary: a full volume, an
//! unplugged drive, a process killed mid-write. A temporary file in the
//! target's own directory followed by a rename has neither hole — the rename is
//! atomic within a filesystem, and every way the write can fail leaves the
//! original exactly as it was. The directory has to be the target's own, not
//! the system temporary directory, because a rename across filesystems is not a
//! rename at all: it degrades to copy-then-delete, which is the truncate window
//! again under a different name.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::dialog;

/// What a file looked like when the shell last agreed with it.
///
/// Length **and** modification time, because either alone misses a real case: a
/// spreadsheet edited by another application very often keeps its length, and a
/// filesystem with a coarse mtime can report the same instant for two writes a
/// moment apart. Together they catch what `docs/83` §5.3–5.4 ask them to — a
/// second window on the same file, a sync client, another application — without
/// a lock file, which strands documents when a process dies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    /// `None` where the platform does not report one. Absent on both sides
    /// compares equal, which is the right answer: an unavailable clock is not
    /// evidence that the file changed.
    modified: Option<SystemTime>,
}

impl Stamp {
    /// What the file at `path` looks like now.
    pub fn of(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// Why a save did not happen, in terms a shell can put on screen.
///
/// Each variant is a *different thing for the user to do*, which is why they
/// are not one string: a read-only file wants a Save As, a changed file wants a
/// decision, and a missing directory wants a different folder.
#[derive(Debug)]
pub enum SaveError {
    /// The file exists and refuses to be written.
    ReadOnly { name: String },
    /// The folder the file was in is not there any more.
    DirectoryGone { dir: String },
    /// The file changed since the shell last agreed with it.
    Changed { name: String },
    /// Anything the platform reported.
    Io { name: String, why: String },
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly { name } => {
                write!(f, "{name} is read-only, so it was not written")
            }
            Self::DirectoryGone { dir } => {
                write!(f, "the folder {dir} is no longer there")
            }
            Self::Changed { name } => {
                write!(f, "{name} changed on disk since it was opened")
            }
            Self::Io { name, why } => write!(f, "could not write {name}: {why}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveError {
    /// The tag the webview switches on. Kept separate from [`Display`], because
    /// a message is for a person and a tag is for a branch — a UI keying off
    /// the prose is one rewording away from breaking.
    ///
    /// [`Display`]: std::fmt::Display
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ReadOnly { .. } => "read-only",
            Self::DirectoryGone { .. } => "no-directory",
            Self::Changed { .. } => "changed",
            Self::Io { .. } => "failed",
        }
    }
}

/// The name of a temporary file that cannot collide with another window's.
///
/// Leading dot so it is hidden while it exists, the process id and a clock
/// reading so two windows saving the same document at the same moment do not
/// write each other's bytes, and `.tmp` last so nothing mistakes it for a
/// workbook.
fn temp_name(path: &Path) -> String {
    let stem = dialog::base_name(&path.to_string_lossy());
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(".{stem}.{}.{nanos}.opencalc-tmp", std::process::id())
}

/// Write `bytes` to `path`, atomically, without losing what is there on the way.
///
/// `expected` is what the shell believed the file looked like; `Some` compares
/// and refuses a file that changed, `None` skips the comparison — which is what
/// `force` amounts to once the user has been asked and has said overwrite.
///
/// Returns the stamp of what was written, so the caller can keep agreeing with
/// the file it just made.
pub fn write_in_place(
    path: &Path,
    bytes: &[u8],
    expected: Option<&Stamp>,
) -> Result<Stamp, SaveError> {
    let name = dialog::base_name(&path.to_string_lossy());
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let Some(dir) = dir else {
        // A bare file name has no directory to write a temporary file into, and
        // the shell only ever holds absolute paths a panel returned. Reaching
        // here is a bug, and it is named rather than guessed at.
        return Err(SaveError::Io {
            name,
            why: "the shell was given a path with no folder in it".to_owned(),
        });
    };
    if !dir.is_dir() {
        return Err(SaveError::DirectoryGone {
            dir: dir.to_string_lossy().into_owned(),
        });
    }

    // What is there now, if anything. A target that has been deleted since it
    // was opened is not an error — the user asked for their work to be at that
    // path and it will be — so `None` here means "create it".
    let existing = fs::metadata(path).ok();
    if let Some(meta) = &existing {
        if meta.is_dir() {
            return Err(SaveError::Io {
                name,
                why: "that path is a folder".to_owned(),
            });
        }
        // **Refused rather than replaced.** On Unix a rename over a read-only
        // file succeeds — the permission that matters is the *directory's* —
        // so an atomic write would silently defeat the flag the user set. The
        // shell checks it here so that the answer is the same on every
        // platform and is the one the user asked for.
        if meta.permissions().readonly() {
            return Err(SaveError::ReadOnly { name });
        }
        if let Some(expected) = expected {
            let now = Stamp {
                len: meta.len(),
                modified: meta.modified().ok(),
            };
            if &now != expected {
                return Err(SaveError::Changed { name });
            }
        }
    }

    let temp = dir.join(temp_name(path));
    // Everything from here to the rename cleans up after itself: a temporary
    // file left in the user's folder is litter they did not ask for and cannot
    // identify.
    let written = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        // Before the rename, not after. A rename that lands while the bytes are
        // still in the page cache is an atomic swap to a file that may be
        // empty after a power loss, which is the failure this whole function
        // exists to rule out.
        file.sync_all()?;
        Ok(())
    })();
    if let Err(why) = written {
        let _ = fs::remove_file(&temp);
        return Err(SaveError::Io {
            name,
            why: why.to_string(),
        });
    }

    // The original's permissions, carried across. Without this a document that
    // was `0600` comes back `0644` after its first save — the file survives and
    // the decision the user made about who can read it does not.
    if let Some(meta) = &existing {
        let _ = fs::set_permissions(&temp, meta.permissions());
    }

    if let Err(why) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(SaveError::Io {
            name,
            why: why.to_string(),
        });
    }

    Stamp::of(path).map_err(|why| SaveError::Io {
        name,
        why: why.to_string(),
    })
}

/// The editor command that replaces the document in the window.
///
/// The editor derives its ids from the English menu labels — `commandId()` in
/// `webapp/editor.selection.js` slugs `File ▸ New` to exactly this — and the
/// operating-system menu dispatches by id, so the shell can see the command go
/// past and drop the save target before it runs.
///
/// **This is a belt, and it is named as one.** The right place for the clear is
/// the editor, in the handler that calls `session_new()`: `newDocument()` in
/// `webapp/editor.sheets.js` exists for it and calls `clearSaveTarget()`. Until
/// the editor's `File ▸ New` calls that, this is what stands between a new
/// workbook and the last document's file. A second definition of one fact
/// drifts, so `tests/browser/editor.save-target.spec.mjs` asserts that this id
/// is still a command the editor has.
pub const NEW_DOCUMENT_COMMAND: &str = "file.new";

/// Does this command id replace the document, and so invalidate the target?
///
/// Erring towards clearing is deliberate: a target dropped when it did not need
/// to be costs the user one Save As panel, and a target kept when it should have
/// gone costs them the file it points at.
pub fn replaces_the_document(id: &str) -> bool {
    id == NEW_DOCUMENT_COMMAND
}

/// The one path this window may write back to, and how it came to be that.
///
/// Two slots rather than one, and the second is the point. `native_open` has a
/// path in hand *before* anybody knows whether the bytes behind it are a
/// workbook this build can read — so the path is **armed**, not adopted, and it
/// becomes the target only when the webview reports that the document on screen
/// is that file. An open that fails to parse leaves the previous document on
/// screen, and a target adopted eagerly would point `Ctrl+S` at the new file
/// while the old document was still in the window: one keystroke from
/// overwriting a file the user never opened.
///
/// The promotion signal is the document name the webview already pushes for the
/// title bar (`set_document`), so nothing new has to be remembered to send.
#[derive(Debug, Default)]
pub struct SaveTarget {
    armed: Option<PathBuf>,
    path: Option<PathBuf>,
    stamp: Option<Stamp>,
}

impl SaveTarget {
    /// A panel returned this path and its bytes have been handed over. It is a
    /// candidate until the webview says the document is that file.
    pub fn arm(&mut self, path: PathBuf) {
        self.armed = Some(path);
    }

    /// The webview reported which document is on screen.
    ///
    /// Promotes an armed candidate whose name matches, and drops one whose name
    /// does not — a report naming the *previous* document is an open that did
    /// not take, and a candidate that survives it would be adopted later by
    /// accident.
    pub fn observe_document(&mut self, name: Option<&str>) {
        let Some(armed) = self.armed.take() else {
            return;
        };
        let matches = name.is_some_and(|n| n == dialog::base_name(&armed.to_string_lossy()));
        if matches {
            // A stamp is taken at adoption rather than at `arm`, so the window
            // it is compared against starts when the document is on screen.
            self.stamp = Stamp::of(&armed).ok();
            self.path = Some(armed);
        }
    }

    /// A save panel wrote the document here, so this is what `Ctrl+S` commits to
    /// from now on.
    ///
    /// Only for a save that *is* the document. A `.csv` export of one sheet is
    /// not a rename and must not move the target — the same reason `native_save`
    /// does not touch the document name.
    pub fn adopt(&mut self, path: PathBuf, stamp: Option<Stamp>) {
        self.armed = None;
        self.stamp = stamp;
        self.path = Some(path);
    }

    /// There is no document any more, or it is a different one.
    ///
    /// `File ▸ New` is the case this exists for, and missing it is how a new
    /// workbook overwrites the file the window was showing a moment ago.
    pub fn clear(&mut self) {
        self.armed = None;
        self.path = None;
        self.stamp = None;
    }

    /// The path this window commits to, if it has one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Write the document back to the target.
    ///
    /// `force` skips the changed-file comparison, and is only ever true because
    /// the user was shown the conflict and chose to overwrite.
    ///
    /// `Ok(None)` means there is no target — the caller acquires one through a
    /// panel rather than downloading, which is the whole of the
    /// `opencalc (1).xlsx` problem.
    pub fn write(&mut self, bytes: &[u8], force: bool) -> Result<Option<String>, SaveError> {
        let Some(path) = self.path.clone() else {
            return Ok(None);
        };
        let expected = if force { None } else { self.stamp.as_ref() };
        let stamp = write_in_place(&path, bytes, expected)?;
        self.stamp = Some(stamp);
        Ok(Some(dialog::base_name(&path.to_string_lossy())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory that cleans itself up, so these tests can run anywhere and
    /// leave nothing behind. `std::env::temp_dir` plus the process id and a
    /// counter: two tests in the same binary run on different threads.
    struct Dir(PathBuf);

    impl Dir {
        fn new(what: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "opencalc-save-{}-{what}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a scratch directory");
            Self(path)
        }
        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            // Best effort: a test that made a directory unwritable has already
            // put it back, and a leftover in the temp directory is not worth
            // panicking in a destructor over.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn leftovers(dir: &Dir) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&dir.0)
            .expect("listing the directory")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_bytes_land_at_the_path_that_was_opened() {
        let dir = Dir::new("in-place");
        let file = dir.join("figures.xlsx");
        fs::write(&file, b"old").unwrap();

        let stamp = write_in_place(&file, b"new bytes", None).expect("the write lands");
        assert_eq!(fs::read(&file).unwrap(), b"new bytes");
        assert_eq!(stamp, Stamp::of(&file).unwrap());
        // And nothing else: an in-place save that leaves a second file beside
        // the first is the `opencalc (1).xlsx` problem with a different name.
        assert_eq!(leftovers(&dir), ["figures.xlsx"]);
    }

    #[test]
    fn a_target_writes_back_and_a_window_without_one_says_so() {
        let dir = Dir::new("target");
        let file = dir.join("budget.xlsx");
        fs::write(&file, b"opened").unwrap();

        let mut target = SaveTarget::default();
        assert!(
            target
                .write(b"anything", false)
                .expect("no target is not an error")
                .is_none(),
            "a document with no target is acquired, not downloaded",
        );

        target.arm(file.clone());
        target.observe_document(Some("budget.xlsx"));
        assert_eq!(target.path(), Some(file.as_path()));
        assert_eq!(
            target.write(b"saved once", false).unwrap().as_deref(),
            Some("budget.xlsx")
        );
        assert_eq!(fs::read(&file).unwrap(), b"saved once");
        // Twice in a row: the second save must not trip its own first one's
        // change detection, which it would if the stamp were not refreshed.
        assert_eq!(
            target.write(b"saved twice", false).unwrap().as_deref(),
            Some("budget.xlsx")
        );
        assert_eq!(fs::read(&file).unwrap(), b"saved twice");
    }

    #[test]
    fn an_open_that_did_not_take_leaves_the_previous_target_alone() {
        let dir = Dir::new("armed");
        let good = dir.join("figures.xlsx");
        let bad = dir.join("not-a-workbook.xlsx");
        fs::write(&good, b"figures").unwrap();
        fs::write(&bad, b"garbage").unwrap();

        let mut target = SaveTarget::default();
        target.arm(good.clone());
        target.observe_document(Some("figures.xlsx"));
        assert_eq!(target.path(), Some(good.as_path()));

        // The panel returned a second file, the engine refused it, and the
        // webview goes on reporting the document that is still on screen.
        target.arm(bad.clone());
        target.observe_document(Some("figures.xlsx"));
        assert_eq!(
            target.path(),
            Some(good.as_path()),
            "an open that failed must not move the target",
        );
        target.write(b"still figures", false).unwrap();
        assert_eq!(fs::read(&good).unwrap(), b"still figures");
        assert_eq!(fs::read(&bad).unwrap(), b"garbage", "untouched");
    }

    #[test]
    fn the_command_that_replaces_the_document_is_the_one_the_editor_names() {
        // The id the operating-system menu dispatches. Asserted on this side so
        // that a rename is a compile-and-test event rather than a save target
        // that quietly stops being cleared; asserted on the *editor's* side by
        // `tests/browser/editor.save-target.spec.mjs`, which is what proves the
        // id still exists there.
        assert!(replaces_the_document("file.new"));
        assert!(!replaces_the_document("file.open"));
        assert!(!replaces_the_document(
            "file.download.same-format-as-opened"
        ));
        assert!(!replaces_the_document(""));
    }

    #[test]
    fn a_new_document_does_not_inherit_the_last_ones_file() {
        // `docs/83` §3.2: "Missing that clear is how a new document overwrites
        // the last one, and it is the acceptance test in §8."
        let dir = Dir::new("cleared");
        let file = dir.join("figures.xlsx");
        fs::write(&file, b"a year of work").unwrap();

        let mut target = SaveTarget::default();
        target.arm(file.clone());
        target.observe_document(Some("figures.xlsx"));
        target.clear();

        assert_eq!(target.path(), None);
        assert!(
            target.write(b"", false).unwrap().is_none(),
            "nothing is written"
        );
        assert_eq!(fs::read(&file).unwrap(), b"a year of work");
    }

    #[test]
    fn a_second_window_cannot_overwrite_the_first_without_being_told() {
        // Two windows on one file, `docs/83` §5.3. Detection, not a lock: the
        // second to save is the one that is told.
        let dir = Dir::new("two-windows");
        let file = dir.join("shared.xlsx");
        fs::write(&file, b"opened by both").unwrap();

        let mut first = SaveTarget::default();
        first.arm(file.clone());
        first.observe_document(Some("shared.xlsx"));
        let mut second = SaveTarget::default();
        second.arm(file.clone());
        second.observe_document(Some("shared.xlsx"));

        second.write(b"the second window's version", false).unwrap();

        let refused = first
            .write(b"the first window's version", false)
            .expect_err("the file changed underneath");
        assert_eq!(refused.kind(), "changed");
        assert!(refused.to_string().contains("shared.xlsx"), "{refused}");
        assert_eq!(
            fs::read(&file).unwrap(),
            b"the second window's version",
            "a refused save writes nothing",
        );

        // …and the user chooses to overwrite.
        first.write(b"the first window's version", true).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"the first window's version");
        // The stamp moved with the forced write, so the next ordinary save is
        // not refused by the conflict this one resolved.
        first.write(b"and again", false).expect("no stale conflict");
    }

    #[test]
    fn a_file_changed_by_another_application_is_refused() {
        // §5.4: a sync client, another editor, the user's own second window all
        // present identically. A same-length change is the case a length-only
        // check would miss.
        let dir = Dir::new("changed");
        let file = dir.join("figures.xlsx");
        fs::write(&file, b"aaaa").unwrap();
        let stamp = Stamp::of(&file).unwrap();
        // Slept rather than assumed: a filesystem whose mtime is coarse would
        // otherwise report the same instant and the length is deliberately the
        // same in this test.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&file, b"bbbb").unwrap();

        let refused = write_in_place(&file, b"cccc", Some(&stamp)).expect_err("changed");
        assert_eq!(refused.kind(), "changed");
        assert_eq!(fs::read(&file).unwrap(), b"bbbb");
    }

    #[test]
    fn a_read_only_file_is_refused_rather_than_replaced() {
        let dir = Dir::new("read-only");
        let file = dir.join("locked.xlsx");
        fs::write(&file, b"protected").unwrap();
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file, perms).unwrap();

        let refused = write_in_place(&file, b"new", None).expect_err("read-only");
        assert_eq!(refused.kind(), "read-only");
        assert!(refused.to_string().contains("locked.xlsx"), "{refused}");
        assert_eq!(fs::read(&file).unwrap(), b"protected");
        assert_eq!(
            leftovers(&dir),
            ["locked.xlsx"],
            "no temporary file left behind"
        );

        // Put it back so the scratch directory can be removed on Windows,
        // where a read-only entry refuses to be deleted. Not through
        // `set_readonly(false)`, which on Unix means world-writable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        #[cfg(windows)]
        {
            let mut perms = fs::metadata(&file).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            fs::set_permissions(&file, perms).unwrap();
        }
    }

    #[test]
    fn a_directory_that_is_gone_is_named() {
        let dir = Dir::new("gone");
        let file = dir.join("removed/figures.xlsx");
        let refused = write_in_place(&file, b"new", None).expect_err("no directory");
        assert_eq!(refused.kind(), "no-directory");
        assert!(refused.to_string().contains("removed"), "{refused}");
    }

    #[test]
    fn a_target_that_was_deleted_is_written_again_rather_than_refused() {
        // The user asked for their work to be at that path. A file somebody
        // moved to the wastebasket mid-session is not a reason to refuse to
        // save; it is a reason to put it back.
        let dir = Dir::new("deleted");
        let file = dir.join("figures.xlsx");
        fs::write(&file, b"old").unwrap();
        let stamp = Stamp::of(&file).unwrap();
        fs::remove_file(&file).unwrap();

        write_in_place(&file, b"restored", Some(&stamp)).expect("written afresh");
        assert_eq!(fs::read(&file).unwrap(), b"restored");
    }

    #[cfg(unix)]
    #[test]
    fn a_write_that_fails_leaves_the_old_file_and_no_debris() {
        // The rule `docs/83` §5.1–5.2 exists for: a save must never leave the
        // user with neither the old file nor the new one. An unwritable
        // directory is how that is provoked without a full disk.
        use std::os::unix::fs::PermissionsExt;
        let dir = Dir::new("failing");
        let file = dir.join("figures.xlsx");
        fs::write(&file, b"a year of work").unwrap();

        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o555)).unwrap();
        // Root ignores the directory's mode, so this provocation does not
        // provoke anything in a container that runs as uid 0. Said out loud
        // rather than passing vacuously: a test that asserts nothing on the
        // machine CI uses is worse than one that is absent.
        if fs::File::create(dir.join(".writable-probe")).is_ok() {
            let _ = fs::remove_file(dir.join(".writable-probe"));
            fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipped: this process can write a mode-0555 directory (running as root?)");
            return;
        }
        let refused = write_in_place(&file, b"new bytes", None).expect_err("the write fails");
        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(refused.kind(), "failed");
        assert!(refused.to_string().contains("figures.xlsx"), "{refused}");
        assert_eq!(
            fs::read(&file).unwrap(),
            b"a year of work",
            "the file the user had is untouched",
        );
        assert_eq!(
            leftovers(&dir),
            ["figures.xlsx"],
            "no temporary file left behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_files_own_permissions_survive_a_save() {
        // A document that was readable only by its owner must not come back
        // world-readable because the shell wrote it through a fresh file.
        use std::os::unix::fs::PermissionsExt;
        let dir = Dir::new("modes");
        let file = dir.join("private.xlsx");
        fs::write(&file, b"secrets").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();

        write_in_place(&file, b"more secrets", None).expect("the write lands");
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the mode came across with the bytes");
    }
}
