//! What a distributable has to carry, and how to notice when it does not.
//!
//! `frontendDist` is `../webapp`, and `tauri::generate_context!` **embeds that
//! directory into the binary at compile time**. Which means the assets are
//! chosen by whatever happens to be in the source tree when `cargo build` runs,
//! and the one thing that is not in the source tree is `webapp/pkg/` — the
//! WebAssembly engine, produced by `wasm-pack` and git-ignored.
//!
//! The failure mode is the reason this file exists: nothing errors. The build
//! succeeds, the bundle is produced, the application launches, the window
//! opens, and the editor's first `import("./pkg/casual_calc_wasm.js")` 404s
//! against an embedded asset store that never had it. A white window, in a
//! shipped installer, from a green build.
//!
//! So the check is a build-time one, and it is shared: `build.rs` includes this
//! file directly rather than restating it, because two copies of a list of
//! required files is one copy that drifts.

use std::path::{Path, PathBuf};

/// Files the webview cannot boot without, relative to `frontendDist`.
///
/// The engine and its loader, and the page that loads them. Not a full
/// manifest of `webapp/` — a missing stylesheet is ugly, a missing `.wasm` is a
/// blank window, and only the second is worth failing a build over.
pub const REQUIRED_WEB_ASSETS: &[&str] = &[
    "editor.html",
    "editor.js",
    "pkg/casual_calc_wasm.js",
    "pkg/casual_calc_wasm_bg.wasm",
];

/// Which of [`REQUIRED_WEB_ASSETS`] are not in `dist`.
///
/// Returned rather than reported, so the caller decides whether a missing file
/// is a warning (a developer build, which can be run from a tree they are about
/// to populate) or an error (a release build, which is about to be handed to
/// somebody who cannot).
pub fn missing_web_assets(dist: &Path) -> Vec<PathBuf> {
    REQUIRED_WEB_ASSETS
        .iter()
        .map(|rel| dist.join(rel))
        .filter(|path| !path.exists())
        .collect()
}

/// What to tell somebody whose build is missing the engine.
///
/// The command, not a description of it. A build error that says "run the wasm
/// build" costs a search through three READMEs; this one is copy-pasteable.
pub fn how_to_build_web_assets() -> &'static str {
    "wasm-pack build crates/casual-calc-wasm --release --target web \
     --out-dir \"$PWD/webapp/pkg\" (see webapp/README.md)"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch directory of our own; no dev-dependency for four lines.
    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opencalc-desktop-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_empty_dist_is_missing_everything() {
        let dir = scratch("empty");
        let missing = missing_web_assets(&dir);
        assert_eq!(missing.len(), REQUIRED_WEB_ASSETS.len());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tree_without_the_wasm_build_is_caught() {
        // Exactly the state of a fresh checkout: every hand-written file is
        // present and `pkg/` — a build artifact, git-ignored — is not. This is
        // the case that builds green and ships a blank window.
        let dir = scratch("no-pkg");
        fs::write(dir.join("editor.html"), "<!doctype html>").unwrap();
        fs::write(dir.join("editor.js"), "// editor").unwrap();
        let missing = missing_web_assets(&dir);
        let names: Vec<String> = missing
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "pkg/casual_calc_wasm.js".to_owned(),
                "pkg/casual_calc_wasm_bg.wasm".to_owned()
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_complete_tree_is_not_complained_about() {
        let dir = scratch("complete");
        fs::create_dir_all(dir.join("pkg")).unwrap();
        for rel in REQUIRED_WEB_ASSETS {
            fs::write(dir.join(rel), "x").unwrap();
        }
        assert!(missing_web_assets(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_remedy_is_a_command_somebody_can_run() {
        let how = how_to_build_web_assets();
        assert!(how.starts_with("wasm-pack build crates/casual-calc-wasm"));
        assert!(how.contains("webapp/pkg"));
    }
}
