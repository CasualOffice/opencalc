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

// The window is the point of this binary; there is nothing to print.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use casual_calc_desktop::dialog;
use casual_calc_desktop::menu::{self, Menu as MenuModel, Node};
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
#[derive(Default)]
struct Shell {
    session: Mutex<Session>,
    opened: Mutex<Option<Vec<u8>>>,
    staged: Mutex<Option<Vec<u8>>>,
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
#[tauri::command]
fn set_document(window: WebviewWindow, name: Option<String>, dirty: bool) -> Result<(), String> {
    let title = {
        let shell = window.state::<Shell>();
        let mut session = locked(&shell.session)?;
        session.set_document(name, dirty);
        session.title()
    };
    window.set_title(&title).map_err(|why| why.to_string())
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
    {
        let shell = app.state::<Shell>();
        let session = locked(&shell.session)?;
        session.guard_open()?;
    }

    let mut panel = app.dialog().file().set_title("Open");
    for filter in dialog::open_filters() {
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
#[tauri::command]
async fn native_save(app: AppHandle, ext: String) -> Result<Option<String>, String> {
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
    std::fs::write(&path, &bytes).map_err(|why| format!("could not write the file: {why}"))?;
    Ok(Some(dialog::base_name(&path.to_string_lossy())))
}

/// The shell's half of the bridge, injected into the page.
///
/// It is *here* rather than in `webapp/` on purpose. Everything it does is
/// meaningless in a browser tab — there is no `invoke`, no panel, no title bar
/// — so the editor would have to guard every call with a check for a host that
/// only ever exists in this binary. Instead the shell installs the thing it can
/// provide, and the editor asks whether `window.__opencalcNative` is there.
///
/// Five functions, and each is a native capability rather than a policy:
/// `open()` raises the panel and returns bytes, `save()` writes them,
/// `setDocument()` moves the title bar, `syncCapabilities()` re-reports what
/// the mode allows, `publishMenu()` rebuilds the bar. *When* to call them is
/// the editor's decision, made where the editor's own rules live.
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
      await invoke("set_capabilities", { capabilities: caps });
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
    async save(bytes, ext) {
      await native.syncCapabilities();
      await invoke("stage_save_bytes", new Uint8Array(bytes));
      return await invoke("native_save", { ext: String(ext) });
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
        ])
        .on_menu_event(|app, event| {
            // The id is the editor's own command id, so this is the whole of
            // the dispatch: no second table, no mapping to keep in step.
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
