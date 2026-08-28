//! The build script, and the one thing it refuses to let through.
//!
//! `frontendDist` is `../webapp` and `tauri::generate_context!` embeds that
//! directory into the binary. The webview's assets therefore travel with the
//! app for free — except `webapp/pkg/`, which is `wasm-pack` output, is
//! git-ignored, and is missing from every fresh checkout. Nothing errors when
//! it is absent: the build is green, the bundle is produced, the window opens,
//! and the editor's first import 404s against an asset store that never had the
//! engine. A blank window, from a passing build.
//!
//! So a release build that is missing the engine fails here, where somebody can
//! still fix it. A debug build only warns — a developer may well be about to
//! run `wasm-pack` into a tree they are still setting up, and a hard error
//! there would make `cargo test` depend on a toolchain none of the tests need.

// The list of required files, shared with the library rather than restated:
// two copies of "what a distributable needs" is one copy that drifts.
#[path = "src/assets.rs"]
mod assets;

use std::path::Path;

/// Set to skip the check — for a build that is deliberately producing a binary
/// nobody will run, such as a lint or a dependency audit in CI.
const OVERRIDE: &str = "OPENCALC_SKIP_ASSET_CHECK";

fn main() {
    let dist = Path::new("../webapp");
    println!("cargo:rerun-if-changed=src/assets.rs");
    println!("cargo:rerun-if-env-changed={OVERRIDE}");
    // The engine is generated, so a build that ran before it existed has to be
    // invalidated when it appears — otherwise the first build after
    // `wasm-pack` embeds the same assets it embedded before.
    for rel in assets::REQUIRED_WEB_ASSETS {
        println!("cargo:rerun-if-changed=../webapp/{rel}");
    }

    let missing = assets::missing_web_assets(dist);
    if !missing.is_empty() && std::env::var_os(OVERRIDE).is_none() {
        let names: Vec<String> = missing
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>();
        let list = names.join(", ");
        let how = assets::how_to_build_web_assets();
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!(
                "the webview assets this app embeds are missing: {list}\n\
                 A release build without them produces an application that opens a blank \
                 window. Build them first:\n  {how}\n\
                 Set {OVERRIDE}=1 to build anyway."
            );
        }
        println!(
            "cargo:warning=missing webview assets ({list}); the app will open a blank window. \
             Build them with: {how}"
        );
    }

    tauri_build::build();
}
