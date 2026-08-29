//! The desktop application: a window, an operating-system menu, the platform's
//! own file panels, and a title bar that names the document.
//!
//! A desktop app should behave like a desktop app, which means the menu bar
//! belongs to the operating system rather than to a strip of HTML inside the
//! window. `?chrome=native` tells the editor to hand its bar over before first
//! paint, and gives the height back to the sheet.
//!
//! **The menu is not defined here.** It is asked for. The editor funnels every
//! menu item and toolbar button through one command id, so this holds ids and
//! nothing else: `menuModel()` returns the File/Edit/View tree derived from the
//! live DOM, and a click sends the id back through `runCommand(id)`. Two
//! definitions of one menu drift, and the copy that drifts is always the one
//! nobody is looking at — so there is only ever one.
//!
//! The same division runs through everything added since (`UX-DESK-01`). The
//! shell owns what only a native application can do — a title bar, a file
//! panel, a byte written to a path — and the editor owns every decision about
//! *when*. The bridge between them is [`BOOTSTRAP`]: `window.__opencalcNative`,
//! four functions, injected by the shell so that the webview half is a call
//! rather than a second implementation.
//!
//! **Bytes cross this bridge, never paths.** `lib.rs` states it as the shell's
//! invariant and it is enforced here by shape: no command accepts a path and no
//! command hands one out, so a webview cannot ask this process to read a file
//! the user did not choose in a panel.
//!
//! `SAVE-02` restates that more precisely rather than relaxing it: **a path may
//! cross *into* this process only from a platform panel, and never crosses back
//! out.** The shell now remembers one — the file this window was opened from —
//! so that `Ctrl+S` writes the document back to it instead of producing
//! `opencalc (1).xlsx` beside it ([`docs/83`] §3.2). `native_save_target` takes
//! no path and returns only a base name, so the shape is unchanged: the webview
//! still cannot name a destination, only "the one the user already chose".
//!
//! [`docs/83`]: ../../docs/83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md

// The window is the point of this binary; there is nothing to print.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use casual_calc_desktop::dialog;
use casual_calc_desktop::menu::{self, Menu as MenuModel, Node};
use casual_calc_desktop::save::{SaveTarget, write_in_place};
use casual_calc_desktop::session::{Capabilities, Session};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

/// Everything the shell remembers between commands.
///
/// One window, one of these. The two byte slots are hand-offs rather than
/// storage: a file the platform's panel just read and the webview has not
/// collected yet, and a workbook the webview has serialised and the panel has
/// not been chosen for yet. Both are taken rather than read, so a cancelled
/// dialog leaves nothing behind and a second save cannot write the first one's
/// bytes.
///
/// `target` is the exception and deliberately so: it is the file this window
/// commits to, and the point of it is that it *persists*. `take`-semantics
/// would make `Ctrl+S` work once. What it costs is the one piece of shell state
/// that has to be actively cleared — on `File ▸ New`, and on an open that did
/// not take — and [`SaveTarget`] holds that reasoning.
#[derive(Default)]
struct Shell {
    session: Mutex<Session>,
    opened: Mutex<Option<Vec<u8>>>,
    staged: Mutex<Option<Vec<u8>>>,
    target: Mutex<SaveTarget>,
}

/// A poisoned lock is a panic somewhere else; say so rather than panicking too.
fn locked<T>(what: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, String> {
    what.lock()
        .map_err(|_| "the shell's state was left inconsistent by an earlier panic".to_owned())
}

/// What a native Open found, minus the bytes.
///
/// The bytes come back over the raw IPC channel through [`take_opened_bytes`],
/// because a `Vec<u8>` inside a JSON response is serialised as one number per
/// byte — a five-megabyte workbook becomes a twenty-megabyte string, parsed on
/// the thread that also has to draw.
#[derive(Serialize)]
struct Opened {
    name: String,
    size: usize,
}

/// Build the platform menu from the editor's own model.
fn build_menu(app: &AppHandle, model: &[MenuModel]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    // macOS puts the application menu first and expects Quit to live in it, not
    // in File. Adding it here rather than teaching the editor about platforms.
    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::new(app, "OpenCalc", true)?;
        app_menu.append(&PredefinedMenuItem::about(app, None, None)?)?;
        app_menu.append(&PredefinedMenuItem::separator(app)?)?;
        app_menu.append(&PredefinedMenuItem::quit(app, None)?)?;
        menu.append(&app_menu)?;
    }

    for top in model {
        let sub = Submenu::new(app, &top.label, true)?;
        append_nodes(app, &sub, &top.items)?;
        menu.append(&sub)?;
    }
    Ok(menu)
}

fn append_nodes(app: &AppHandle, into: &Submenu<tauri::Wry>, nodes: &[Node]) -> tauri::Result<()> {
    for node in nodes {
        match node {
            Node::Separator => into.append(&PredefinedMenuItem::separator(app)?)?,
            Node::Item {
                id,
                label,
                accelerator,
                enabled,
                ..
            } => {
                // The accelerator the editor already displays, translated once
                // — so the menu shows Cmd on macOS and Ctrl elsewhere without
                // this code knowing which machine it is on.
                let accel = accelerator.as_deref().and_then(menu::accelerator);
                let item = MenuItem::with_id(app, id, label, *enabled, accel.as_deref())?;
                into.append(&item)?;
            }
            Node::Submenu { label, items, .. } => {
                let nested = Submenu::new(app, label, true)?;
                append_nodes(app, &nested, items)?;
                into.append(&nested)?;
            }
        }
    }
    Ok(())
}

/// Ask the editor for its menu, once it has one.
#[tauri::command]
fn publish_menu(window: WebviewWindow, model: String) -> Result<(), String> {
    let parsed = menu::parse(&model).map_err(|why| format!("unreadable menu model: {why}"))?;
    let app = window.app_handle().clone();
    let menu = build_menu(&app, &parsed).map_err(|why| why.to_string())?;
    app.set_menu(menu).map_err(|why| why.to_string())?;
    Ok(())
}

/// What this mode allows, as the editor resolved it.
///
/// The webview reports; the shell does not guess. See `session.rs` for why the
/// direction is that way round and why nothing is permitted until it arrives.
#[tauri::command]
fn set_capabilities(window: WebviewWindow, capabilities: Capabilities) -> Result<(), String> {
    let shell = window.state::<Shell>();
    locked(&shell.session)?.set_capabilities(capabilities);
    Ok(())
}

/// The document in this window changed, or its unsaved state did.
///
/// One command for both, because they are one fact about one title bar and two
/// commands would let them disagree — a name updated without a dirty flag shows
/// a clean document that has unsaved work in it.
/// It is also the signal that promotes a save target. `native_open` has a path
/// in hand before anyone knows whether the bytes behind it parse, so the path is
/// *armed* there and adopted here — when the webview says the document on screen
/// is that file. An open the engine refused reports the previous document's
/// name, and the candidate is dropped. See [`SaveTarget`].
#[tauri::command]
fn set_document(window: WebviewWindow, name: Option<String>, dirty: bool) -> Result<(), String> {
    let title = {
        let shell = window.state::<Shell>();
        let mut session = locked(&shell.session)?;
        session.set_document(name.clone(), dirty);
        session.title()
    };
    {
        let shell = window.state::<Shell>();
        locked(&shell.target)?
            .observe_document(name.as_deref().map(str::trim).filter(|n| !n.is_empty()));
    }
    window.set_title(&title).map_err(|why| why.to_string())
}

/// This window is showing a different document now, so it commits to nothing.
///
/// `File ▸ New` is what this is for. Without it the next `Ctrl+S` writes a blank
/// workbook over the file the window was showing a moment ago — the failure
/// [`docs/83`](../../docs/83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md) §3.2 names as
/// the one the acceptance test exists for.
#[tauri::command]
fn clear_save_target(window: WebviewWindow) -> Result<(), String> {
    let shell = window.state::<Shell>();
    locked(&shell.target)?.clear();
    Ok(())
}

/// The platform's open panel, and the bytes behind whatever it returned.
///
/// `async` because the panel is modal and blocking one on the main thread
/// deadlocks the window it is modal to — the plugin says so, and its own
/// example is this signature.
///
/// Returns `Ok(None)` when the user cancels. Cancelling is not an error and a
/// shell that reported it as one would put a message on screen for somebody who
/// changed their mind.
#[tauri::command]
async fn native_open(app: AppHandle) -> Result<Option<Opened>, String> {
    // The guard and the format list are read under one lock: two locks would
    // let a `set_capabilities` land between them and raise a panel whose
    // filters came from a different report than the permission did.
    let extensions = {
        let shell = app.state::<Shell>();
        let session = locked(&shell.session)?;
        session.guard_open()?;
        session.open_extensions()
    };

    let mut panel = app.dialog().file().set_title("Open");
    for filter in dialog::open_filters(&extensions) {
        let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
        panel = panel.add_filter(&filter.name, &extensions);
    }
    let Some(chosen) = panel.blocking_pick_file() else {
        return Ok(None);
    };

    let path = chosen.into_path().map_err(|why| why.to_string())?;
    let bytes = std::fs::read(&path).map_err(|why| format!("could not read the file: {why}"))?;
    let name = dialog::base_name(&path.to_string_lossy());
    let size = bytes.len();
    {
        let shell = app.state::<Shell>();
        *locked(&shell.opened)? = Some(bytes);
        // Armed, not adopted. The bytes have not been parsed yet, and a target
        // adopted here would point `Ctrl+S` at a file the window is not showing
        // if the engine refuses them. `set_document` decides.
        locked(&shell.target)?.arm(path);
    }
    Ok(Some(Opened { name, size }))
}

/// Collect the bytes the last [`native_open`] read.
///
/// A raw response, so the webview receives an `ArrayBuffer` rather than a JSON
/// array of integers. Taken, not read: holding a second copy of the workbook
/// for the life of the window is how a shell doubles its own memory.
#[tauri::command]
fn take_opened_bytes(window: WebviewWindow) -> Result<Response, String> {
    let shell = window.state::<Shell>();
    let bytes = locked(&shell.opened)?
        .take()
        .ok_or_else(|| "no opened file is waiting to be collected".to_owned())?;
    Ok(Response::new(bytes))
}

/// Hand the shell the bytes to write, before a save panel is raised.
///
/// Synchronous and does no work beyond a move: a command that borrows the
/// invoke message cannot be `async`, and this one has to borrow it to reach the
/// raw body. The panel is [`native_save`]'s job for exactly that reason.
///
/// **Both encodings are accepted, and that is not defensiveness.** Tauri sends
/// an `ArrayBuffer` payload over the custom-protocol IPC as a raw body, and
/// falls back to `postMessage` — where the same payload is JSON, one number per
/// byte — whenever the webview blocks the custom protocol. Taking only the raw
/// form would make Save work on this machine and fail on somebody else's, in a
/// path no test here can reach.
#[tauri::command]
fn stage_save_bytes(window: WebviewWindow, request: Request<'_>) -> Result<(), String> {
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(value) => serde_json::from_value::<Vec<u8>>(value.clone())
            .map_err(|_| "stage_save_bytes wants the workbook as an ArrayBuffer".to_owned())?,
    };
    let shell = window.state::<Shell>();
    *locked(&shell.staged)? = Some(bytes);
    Ok(())
}

/// The platform's save panel, and the write behind it.
///
/// The bytes are taken from the staging slot **before** the panel opens: a
/// cancelled save must not leave a copy of the document sitting in the shell,
/// and a second save must not be able to write the first one's bytes to the
/// second one's file.
///
/// The name it proposes is the open document's under the new extension, so
/// `figures.xlsx` exported as CSV proposes `figures.csv` rather than the
/// browser build's `opencalc.csv`.
///
/// Returns the name written, or `Ok(None)` if the user cancelled. The shell
/// deliberately does **not** update its own document name here: the editor
/// decides what a save means for the document it is showing — a `.csv` export
/// of one sheet is not a rename — and one of the two has to be the authority.
///
/// `adopt` is the same question asked about the *save target*, and for the same
/// reason. A `Ctrl+S` on a document that has no target yet acquires one here, so
/// the path this panel returned becomes the file the window commits to. A
/// `File ▸ Download ▸ CSV` writes a copy and must leave the target where it is —
/// otherwise the next `Ctrl+S` writes a workbook into a `.csv` the user asked
/// for as an export. The editor knows which of the two this is; the shell does
/// not, and does not guess.
#[tauri::command]
async fn native_save(app: AppHandle, ext: String, adopt: bool) -> Result<Option<String>, String> {
    // Taken first and unconditionally, so that *every* way out of this function
    // — a refused capability, a cancelled panel, a failed write — leaves no
    // copy of the document behind in the shell.
    let staged = {
        let shell = app.state::<Shell>();
        locked(&shell.staged)?.take()
    };
    let suggested = {
        let shell = app.state::<Shell>();
        let session = locked(&shell.session)?;
        session.guard_save()?;
        session.suggested_save_name(&ext)
    };
    let bytes = staged.ok_or_else(|| "nothing was staged to save".to_owned())?;

    let mut panel = app
        .dialog()
        .file()
        .set_title("Save As")
        .set_file_name(suggested);
    for filter in dialog::save_filters(&ext) {
        let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
        panel = panel.add_filter(&filter.name, &extensions);
    }
    let Some(chosen) = panel.blocking_save_file() else {
        return Ok(None);
    };

    let path = chosen.into_path().map_err(|why| why.to_string())?;
    // The same atomic write the in-place save uses. A Save As over an existing
    // file has exactly the hazard `save::write_in_place` exists for: a partial
    // write leaves the user with neither the file they picked nor the document
    // they were saving. No change check, because the user just named this file
    // in a panel and the platform already asked about replacing it.
    let stamp = write_in_place(&path, &bytes, None).map_err(|why| why.to_string())?;
    let name = dialog::base_name(&path.to_string_lossy());
    if adopt {
        let shell = app.state::<Shell>();
        locked(&shell.target)?.adopt(path, Some(stamp));
    }
    Ok(Some(name))
}

/// What a `Ctrl+S` against the window's own file did.
///
/// Four outcomes rather than a `Result<Option<String>, String>`, because they
/// are four different things for the user to do next and the webview has to
/// branch on them: a changed file wants a decision, a read-only one wants a
/// Save As, a missing folder wants a different folder, and no target at all is
/// not a failure — it is a document that has never been saved, and the answer is
/// to acquire a target rather than to download.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum SavedToTarget {
    /// The bytes are on disk at the path the user opened.
    Written { name: String },
    /// This window has no file to commit to yet.
    NoTarget,
    /// Nothing was written, and this is why.
    Refused {
        kind: &'static str,
        name: String,
        why: String,
    },
}

/// Write the document back to the file this window was opened from.
///
/// The command `SAVE-02` adds, and the reason the "bytes, never paths"
/// invariant survives it: it takes no path, and the only thing it hands back is
/// a base name. `force` is only ever true because the user was shown a
/// changed-file conflict and chose to overwrite.
///
/// `guard_save()` applies, as it does to the panel: a mode without `canSaveAs`
/// cannot reach a file through this door either.
///
/// `async` for the same reason [`native_save`] is, minus the panel: the write
/// blocks, and a blocking write on the thread that also draws is a window that
/// stops repainting for as long as the filesystem takes.
#[tauri::command]
async fn native_save_target(app: AppHandle, force: bool) -> Result<SavedToTarget, String> {
    let shell = app.state::<Shell>();
    // Taken first and unconditionally, exactly as `native_save` does: every way
    // out of this function leaves no copy of the document behind in the shell.
    let staged = locked(&shell.staged)?.take();
    locked(&shell.session)?.guard_save()?;
    let bytes = staged.ok_or_else(|| "nothing was staged to save".to_owned())?;

    let mut target = locked(&shell.target)?;
    match target.write(&bytes, force) {
        Ok(Some(name)) => Ok(SavedToTarget::Written { name }),
        Ok(None) => Ok(SavedToTarget::NoTarget),
        Err(why) => Ok(SavedToTarget::Refused {
            kind: why.kind(),
            name: target
                .path()
                .map(|p| dialog::base_name(&p.to_string_lossy()))
                .unwrap_or_default(),
            why: why.to_string(),
        }),
    }
}

/// The shell's half of the bridge, injected into the page.
///
/// It is *here* rather than in `webapp/` on purpose. Everything it does is
/// meaningless in a browser tab — there is no `invoke`, no panel, no title bar
/// — so the editor would have to guard every call with a check for a host that
/// only ever exists in this binary. Instead the shell installs the thing it can
/// provide, and the editor asks whether `window.__opencalcNative` is there.
///
/// Seven functions, and each is a native capability rather than a policy:
/// `open()` raises the panel and returns bytes, `save()` writes them through
/// one, `saveTarget()` writes them back to the file the window was opened from,
/// `clearSaveTarget()` says this is a different document now, `setDocument()`
/// moves the title bar, `syncCapabilities()` re-reports what the mode allows,
/// `publishMenu()` rebuilds the bar. *When* to call them is the editor's
/// decision, made where the editor's own rules live — which is why the editor,
/// not this bridge, decides that `Ctrl+S` means `saveTarget` and
/// `File ▸ Download` means `save`.
///
/// Injected on **every page load**, not once at startup. A reload — Cmd+R, or
/// anything that navigates — replaces `window`, and a bridge installed once
/// leaves the second page with a menu bar that dispatches into nothing and an
/// Open that does not exist. This is idempotent, so re-running it is the fix.
const BOOTSTRAP: &str = r#"(function () {
  const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
  if (!invoke) return;
  const editor = () => window.opencalcEditor;

  const native = {
    // The mode's answer, pushed to the shell. Re-sent before every panel: a
    // host can change capabilities at any time through `setCapabilities`, and a
    // permission the shell was told about at boot is a permission that may have
    // been withdrawn since.
    async syncCapabilities() {
      const e = editor();
      if (!e || !e.getCapabilities) return null;
      const caps = e.getCapabilities();
      // The engine's own answer about what it can open, carried on the same
      // report so the panel and the permission can never come from different
      // moments. `openable_extensions` asks the SDK about candidates rather
      // than reciting a list, so a format the engine learns shows up in the
      // panel the day it does — which is why the shell stopped keeping a list.
      let openExtensions = [];
      try {
        openExtensions = JSON.parse(e.wasmApi().openable_extensions())
          .map((x) => String(x).replace(/^[."']+|["']+$/g, ""));
      } catch (err) {
        // A shell that cannot read the list falls back to the floor, not to
        // nothing: an Open panel with no filters opens no files at all.
        console.error("[opencalc] openable_extensions", err);
      }
      await invoke("set_capabilities", { capabilities: { ...caps, openExtensions } });
      return caps;
    },
    async setDocument(name, dirty) {
      await invoke("set_document", { name: name == null ? null : String(name), dirty: !!dirty });
    },
    // Rebuild the operating system's menu bar from the editor's live DOM. The
    // model carries what is hidden and what is disabled, so anything that
    // changes those — read-only, a mode change, a capability a host withdrew —
    // leaves the native bar stale until this is called.
    async publishMenu() {
      const e = editor();
      if (!e || !e.menuModel) return;
      await invoke("publish_menu", { model: JSON.stringify(e.menuModel()) });
    },
    // Returns { name, bytes } or null when the user cancelled.
    async open() {
      await native.syncCapabilities();
      const opened = await invoke("native_open");
      if (!opened) return null;
      const bytes = await invoke("take_opened_bytes");
      return { name: opened.name, bytes: new Uint8Array(bytes) };
    },
    // Returns the file name written, or null when the user cancelled.
    //
    // `adopt` says whether the file the user picks *becomes* this window's save
    // target. A `Ctrl+S` on a document that has never been saved acquires one
    // here and passes true; `File ▸ Download ▸ CSV` writes a copy and passes
    // false, because a `.csv` export of one sheet is not where the workbook
    // lives now.
    async save(bytes, ext, adopt) {
      await native.syncCapabilities();
      await invoke("stage_save_bytes", new Uint8Array(bytes));
      return await invoke("native_save", { ext: String(ext), adopt: !!adopt });
    },
    // Write the document back to the file this window was opened from.
    //
    // Returns `{status}`: `written` with the name, `no-target` when this window
    // has no file yet — the caller acquires one through `save()` rather than
    // downloading — or `refused` with a `kind` (`changed`, `read-only`,
    // `no-directory`, `failed`) and a sentence to show. `force` is only ever
    // true because the user was shown a changed-file conflict and chose to
    // overwrite.
    async saveTarget(bytes, force) {
      await native.syncCapabilities();
      await invoke("stage_save_bytes", new Uint8Array(bytes));
      return await invoke("native_save_target", { force: !!force });
    },
    // This window is showing a different document now. `File ▸ New`: without
    // it the next Ctrl+S writes a blank workbook over the file that was open.
    async clearSaveTarget() {
      await invoke("clear_save_target");
    },
  };
  window.__opencalcNative = native;

  // The editor builds its menus during boot, so the model is asked for after
  // the page settles rather than at window creation.
  (function wait() {
    const e = editor();
    if (e && e.menuModel) {
      native.publishMenu().catch((err) => console.error("[opencalc] menu", err));
      native.syncCapabilities().catch((err) => console.error("[opencalc] capabilities", err));
      window.dispatchEvent(new CustomEvent("opencalc-native-ready"));
    } else {
      setTimeout(wait, 60);
    }
  })();
})()"#;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Shell::default())
        .invoke_handler(tauri::generate_handler![
            publish_menu,
            set_capabilities,
            set_document,
            native_open,
            take_opened_bytes,
            stage_save_bytes,
            native_save,
            native_save_target,
            clear_save_target,
        ])
        .on_menu_event(|app, event| {
            // The id is the editor's own command id, so this is the whole of
            // the dispatch: no second table, no mapping to keep in step.
            //
            // One exception, and it is a safety one. `File ▸ New` replaces the
            // document, and a save target that survives it points `Ctrl+S` at
            // the file the window was showing a moment ago. The clear happens
            // *before* the command runs, so a New the user then cancels leaves
            // this window without a target — one Save As panel, which is the
            // cheap side of the mistake. See [`save::NEW_DOCUMENT_COMMAND`] for
            // why this lives here and what replaces it.
            let replaces = casual_calc_desktop::save::replaces_the_document(&event.id().0);
            if let Some(shell) = replaces.then(|| app.try_state::<Shell>()).flatten() {
                // A poisoned lock here means a panic elsewhere; the save target
                // is already unusable and there is nothing to report to.
                let _ = shell.target.lock().map(|mut target| target.clear());
            }
            let id = event.id().0.replace('\\', "\\\\").replace('\'', "\\'");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval(format!(
                    "try {{ window.opencalcEditor.runCommand('{id}') }} \
                     catch (e) {{ console.error('[opencalc] menu', e) }}"
                ));
            }
        })
        .on_page_load(|webview, payload| {
            // Every load, including a reload: see [`BOOTSTRAP`]. `Finished`
            // rather than `Started` because the bridge needs `window.__TAURI__`,
            // and nothing on the page looks for it before then.
            if payload.event() == PageLoadEvent::Finished {
                let _ = webview.eval(BOOTSTRAP);
            }
        })
        .setup(|app| {
            let window = app.get_webview_window("main").expect("the main window");
            // Before the editor has said anything, so the very first frame
            // reads `Untitled — OpenCalc` rather than the product name alone.
            let _ = window.set_title(&Session::default().title());
            let _ = app.emit("ready", ());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the application");
}
