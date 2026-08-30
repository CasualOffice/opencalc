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
//! `TAURI-010` widens *where a path may come from* by exactly one source, and
//! restates the invariant rather than weakening it: **a path may cross into
//! this process only from the platform** — a file panel, or the operating
//! system handing this application a file to open (`argv` on Windows and Linux,
//! `RunEvent::Opened` on macOS) — and it never crosses back out. The webview
//! still cannot name a file; it can only collect the one the platform already
//! named. See [`casual_calc_desktop::launch`] and [`take_pending_open`].
//!
//! [`docs/83`]: ../../docs/83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md

// The window is the point of this binary; there is nothing to print.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use casual_calc_desktop::dialog;
use casual_calc_desktop::launch::{self, PendingOpen};
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
///
/// `pending` is the third hand-off and the one with a *time* problem rather
/// than a memory one: a file the operating system asked this application to
/// open, which on macOS can be delivered before the window exists. See
/// [`PendingOpen`] for why readiness is stored beside it rather than inferred.
#[derive(Default)]
struct Shell {
    session: Mutex<Session>,
    opened: Mutex<Option<Vec<u8>>>,
    staged: Mutex<Option<Vec<u8>>>,
    target: Mutex<SaveTarget>,
    pending: Mutex<PendingOpen>,
    /// Whether a close has already been agreed to (`TAURI-011`).
    ///
    /// `CloseRequested` is synchronous and asking the user is not, so the first
    /// request is always refused and the answer arrives later through
    /// [`agree_to_close`]. This is what stops the second request — the one the
    /// answer triggers — from asking again forever.
    closing: std::sync::atomic::AtomicBool,
    /// The menu model as the editor last published it, and whether a cell is
    /// currently open for editing (`TAURI-012`).
    ///
    /// Kept because the native menu has to be rebuilt when the edit state
    /// changes — the accelerators that collide with editing are released while
    /// an edit is open — and the model is the editor's to describe, not the
    /// shell's to reconstruct.
    menu_model: Mutex<Option<Vec<MenuModel>>>,
    editing: std::sync::atomic::AtomicBool,
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
fn build_menu(
    app: &AppHandle,
    model: &[MenuModel],
    editing: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
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
        append_nodes(app, &sub, &top.items, editing)?;
        menu.append(&sub)?;
    }
    Ok(menu)
}

fn append_nodes(
    app: &AppHandle,
    into: &Submenu<tauri::Wry>,
    nodes: &[Node],
    editing: bool,
) -> tauri::Result<()> {
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
                //
                // **Released while a cell is being edited** (`TAURI-012`): a
                // native accelerator is consumed before the webview sees the
                // key, so an item that overloads a chord Excel gives a second
                // meaning to mid-edit would otherwise shadow it permanently.
                // The item stays in the menu and stays clickable; only its key
                // is let go, and only for as long as the edit lasts.
                let accel = if editing && menu::releases_during_edit(id) {
                    None
                } else {
                    accelerator.as_deref().and_then(menu::accelerator)
                };
                let item = MenuItem::with_id(app, id, label, *enabled, accel.as_deref())?;
                into.append(&item)?;
            }
            Node::Submenu { label, items, .. } => {
                let nested = Submenu::new(app, label, true)?;
                append_nodes(app, &nested, items, editing)?;
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
    let shell = window.state::<Shell>();
    *locked(&shell.menu_model)? = Some(parsed.clone());
    let editing = shell.editing.load(std::sync::atomic::Ordering::Relaxed);
    let app = window.app_handle().clone();
    let menu = build_menu(&app, &parsed, editing).map_err(|why| why.to_string())?;
    app.set_menu(menu).map_err(|why| why.to_string())?;
    Ok(())
}

/// A cell is open for editing, or is not (`TAURI-012`).
///
/// **A native menu accelerator is consumed before the webview sees the key.**
/// In a browser, `Cmd+T` in the middle of a formula reaches the editor and
/// cycles the reference's anchors, which is what Excel's own Mac table says it
/// should do. In this shell the menu ate it first and opened a modal over a
/// half-typed formula — so the desktop build was *worse* than the browser one,
/// not merely different.
///
/// The shell cannot know what a keystroke means; only the editor knows whether
/// a cell is open. So the editor says so here, and the colliding accelerators
/// are released for as long as the edit lasts.
///
/// **The menu is rebuilt, and only when the answer changes.** Rebuilding is the
/// blunt instrument — Tauri has no way to clear one item's accelerator on a
/// live menu — but an edit begins and ends at human pace, not per keystroke, so
/// the cost lands once per edit rather than once per character. The early
/// return is what keeps that true.
#[tauri::command]
fn set_editing(window: WebviewWindow, editing: bool) -> Result<(), String> {
    let shell = window.state::<Shell>();
    let was = shell
        .editing
        .swap(editing, std::sync::atomic::Ordering::Relaxed);
    if was == editing {
        return Ok(());
    }
    let model = locked(&shell.menu_model)?.clone();
    let Some(model) = model else {
        // No menu published yet; the flag is stored and the next publish uses it.
        return Ok(());
    };
    let app = window.app_handle().clone();
    let menu = build_menu(&app, &model, editing).map_err(|why| why.to_string())?;
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

/// Collect the file the operating system asked this application to open.
///
/// The webview's half of `TAURI-010`. It is called once when the bridge becomes
/// ready — which is what marks the shell ready, so a file handed over *later*
/// can be nudged rather than silently queued — and again whenever the shell
/// evaluates [`OPEN_HANDED_OVER`] into the page.
///
/// `Ok(None)` is the ordinary answer: most launches open nothing. It is not an
/// error and must not read as one, because it happens every time the
/// application is started from its icon.
///
/// The bytes go through the same door a panel's do — staged in `opened` and
/// collected by [`take_opened_bytes`] — rather than being returned here, for
/// the reason [`Opened`] gives: a `Vec<u8>` inside a JSON response is one
/// number per byte.
///
/// **This is the only place a path enters the process without a panel**, and it
/// is still not the webview naming one: the path came from the platform, the
/// webview cannot influence which, and no path goes back out. `guard_open`
/// applies exactly as it does to `native_open` — a mode whose host owns the
/// document does not acquire a different one because Finder asked.
///
/// `async` for the reason [`native_save_target`] is, minus the panel: this
/// reads a whole workbook off disk, and a synchronous Tauri command runs on the
/// thread that also draws. A hundred-megabyte file collected there is a window
/// that stops repainting for as long as the filesystem takes — on the very
/// first frame the user ever sees, since this is what a launch calls.
#[tauri::command]
async fn take_pending_open(app: AppHandle) -> Result<Option<Opened>, String> {
    let shell = app.state::<Shell>();
    // Taking marks the webview ready even when nothing is waiting, and it is
    // done before the guard so that a refused open still leaves the shell able
    // to nudge the next one.
    let Some(path) = locked(&shell.pending)?.take() else {
        return Ok(None);
    };
    locked(&shell.session)?.guard_open()?;

    let bytes = std::fs::read(&path).map_err(|why| format!("could not read the file: {why}"))?;
    let name = dialog::base_name(&path.to_string_lossy());
    let size = bytes.len();
    *locked(&shell.opened)? = Some(bytes);
    // Armed, not adopted — the same distinction `native_open` makes, and for
    // the same reason: the bytes have not been parsed yet, and a `Ctrl+S`
    // pointed at a file the window failed to open is worse than no target.
    locked(&shell.target)?.arm(path);
    Ok(Some(Opened { name, size }))
}

/// What the shell evaluates into the page when a file arrives after the webview
/// is already running — a second document double-clicked while the window is
/// open.
///
/// `window.eval` rather than an event, because the dispatch that already works
/// in this file is an eval (`on_menu_event`) and `window.__TAURI__.event` is one
/// more piece of the global API this bridge would have to depend on being
/// present. The first launch does not need this at all: the webview asks on its
/// own as soon as it is ready.
const OPEN_HANDED_OVER: &str = "try { window.__opencalcNative.openHandedOver() } \
     catch (e) { console.error('[opencalc] open-file', e) }";

/// The operating system has asked this application to open `path`.
///
/// Both platform routes end here — `argv` on Windows and Linux, `RunEvent::
/// Opened` on macOS — so there is one policy and not two that drift.
///
/// A path the engine cannot open is dropped in silence. It is not the same as
/// a *panel* returning something unreadable: nobody chose this file in this
/// application, and an error dialog raised by an argument nobody typed is
/// noise. See [`launch::opens_here`].
fn hand_over(app: &AppHandle, path: PathBuf) {
    if !launch::opens_here(&path) {
        return;
    }
    let nudge = {
        let shell = app.state::<Shell>();
        let Ok(mut pending) = shell.pending.lock() else {
            return;
        };
        pending.queue(path)
    };
    // Only when the webview has already collected once. Evaluating into a page
    // that has not installed the bridge yet does nothing at all, which is
    // exactly how a first-launch file gets lost — so that case waits for the
    // webview to come and ask instead.
    if nudge && let Some(window) = app.get_webview_window("main") {
        let _ = window.eval(OPEN_HANDED_OVER);
    }
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
/// Nine functions, and each is a native capability rather than a policy:
/// `open()` raises the panel and returns bytes, `save()` writes them through
/// one, `saveTarget()` writes them back to the file the window was opened from,
/// `clearSaveTarget()` says this is a different document now, `setDocument()`
/// moves the title bar, `syncCapabilities()` re-reports what the mode allows,
/// `publishMenu()` rebuilds the bar, and `openHandedOver()` /
/// `disownHandedOver()` collect the file the operating system asked this
/// application to open — or say that it did not become the document.
/// *When* to call them is the editor's
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
    // A cell is open for editing, or is not (`TAURI-012`). A native menu
    // accelerator is consumed before the webview sees the key, so the shell
    // releases the chords Excel overloads for as long as an edit lasts. Cheap
    // to call — the command returns immediately unless the answer changed.
    async setEditing(editing) {
      await invoke("set_editing", { editing: !!editing });
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
    // Open the file the operating system handed this application, if it handed
    // one over (`TAURI-010`).
    //
    // Called twice for two different moments and the second is the one that is
    // usually missing: once by the ready loop below, which is a launch by
    // double-click, and once per `window.eval` from the shell, which is a
    // *second* file double-clicked while this window is already open. A build
    // that only does the first works perfectly until somebody opens a second
    // spreadsheet, and then does nothing with no error anywhere.
    //
    // The body is the `#tb-open` handler's, minus the panel: the same dirty
    // check, the same `openBytes`, the same `setDocument`. Deliberately here in
    // the bridge rather than in `editor.core.js` — there is nothing for a
    // browser tab to do with a file the operating system handed over, so the
    // editor would have to guard every line of it against a host that only
    // exists in this binary.
    async openHandedOver() {
      const e = editor();
      if (!e || !e.openBytes) return;
      // Before the invoke: the shell's `guard_open` reads the last report, and
      // this is reachable at any time — including before the ready loop has
      // sent one, if the shell nudges an already-loaded page.
      await native.syncCapabilities();
      const handed = await invoke("take_pending_open");
      if (!handed) return; // the ordinary launch: nothing was handed over
      const bytes = new Uint8Array(await invoke("take_opened_bytes"));
      // A second activation can land on a window with unsaved work in it. The
      // first launch never asks, because a blank document is not dirty.
      if (e.isDirty && e.isDirty() && e.confirmModal) {
        const ok = await e.confirmModal(
          "Open " + handed.name + "?",
          "This workbook has changes that have not been saved. Opening another discards them, and undo will not bring them back.",
          "Discard and open",
        );
        if (!ok) return await native.disownHandedOver();
      }
      if (e.openBytes(bytes, handed.name)) {
        e.markSaved();
        await native.setDocument(handed.name, false);
      } else {
        // `openBytes` has already put the engine's sentence in the status bar.
        // What it cannot do is tell the shell, and the shell has a path armed.
        await native.disownHandedOver();
      }
    },
    // Say that the handed-over file did **not** become this document.
    //
    // `take_pending_open` arms a save target the way `native_open` does, and
    // `set_document` is what promotes or drops it. But the shell is only told
    // when the name or the dirty flag *changes*, and neither does when the open
    // is declined or refused — so the armed candidate would sit there until
    // something else moved it. Re-reporting the document that is actually on
    // screen is the drop: `SaveTarget::observe_document` discards a candidate
    // whose name does not match what the webview says it is showing.
    async disownHandedOver() {
      const e = editor();
      if (!e || !e.documentName) return;
      await native.setDocument(e.documentName(), !!(e.isDirty && e.isDirty()));
    },
  };
  window.__opencalcNative = native;

  // The editor builds its menus during boot, so the model is asked for after
  // the page settles rather than at window creation.
  //
  // **A non-empty model, not merely the function.** `window.opencalcEditor` is
  // the module namespace and appears the moment the WebAssembly binary loads —
  // which is *before* `wasm.session_new()`, with an `await` in between for the
  // host's fonts. Waiting on `e.menuModel` alone could therefore fire in that
  // gap, and `openHandedOver()` there would open the user's file into a session
  // that boot then replaces with an empty one: the file opens, and a moment
  // later the window is blank, which is this row's defect arriving by a
  // different road. `menuModel()` reads the live DOM and `#menubar` is empty
  // until `buildMenuBar()`, which runs after `session_new()` — so a model with
  // entries in it is the signal that the editor is actually up.
  (function wait() {
    const e = editor();
    let ready = false;
    try {
      ready = !!(e && e.menuModel && e.menuModel().length);
    } catch (err) {
      ready = false;
    }
    if (ready) {
      native.publishMenu().catch((err) => console.error("[opencalc] menu", err));
      native.syncCapabilities().catch((err) => console.error("[opencalc] capabilities", err));
      // The file this application was launched to open, if it was launched to
      // open one. After `syncCapabilities` is *requested* but not awaited —
      // `openHandedOver` re-syncs before it asks the shell for anything, so the
      // guard reads a fresh report either way.
      native.openHandedOver().catch((err) => console.error("[opencalc] open-file", err));
      window.dispatchEvent(new CustomEvent("opencalc-native-ready"));
    } else {
      setTimeout(wait, 60);
    }
  })();
})()"#;

/// The user has answered the close question; let the next request through.
///
/// Split from the window event because **`CloseRequested` cannot wait**. It is
/// a synchronous callback on the event loop, and the question — "you have
/// unsaved work, really close?" — is answered in the webview, asynchronously.
/// So the first request is refused outright, the webview is asked, and this is
/// how the answer comes back.
///
/// Only ever sets the latch to true. A cancelled close simply never calls this,
/// which leaves the window open with nothing to undo.
#[tauri::command]
fn agree_to_close(
    app: tauri::AppHandle,
    shell: tauri::State<'_, Shell>,
    quit: bool,
) -> Result<(), String> {
    use tauri::Manager as _;
    shell
        .closing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // **What was asked decides what happens** (`TAURI-014`). Closing a window
    // and quitting the application are two different requests, and answering
    // one with the other is its own defect: a user who pressed Cmd+Q and got a
    // closed window with the process still running has been ignored, and one
    // who clicked the close button and lost the application has lost more than
    // they asked to.
    if quit {
        // The exit event fires again, sees the latch, and lets it through.
        app.exit(0);
    } else if let Some(window) = app.get_webview_window("main") {
        window.close().map_err(|why| why.to_string())?;
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Shell::default())
        .invoke_handler(tauri::generate_handler![
            publish_menu,
            set_editing,
            set_capabilities,
            set_document,
            native_open,
            take_opened_bytes,
            stage_save_bytes,
            native_save,
            native_save_target,
            clear_save_target,
            take_pending_open,
            agree_to_close,
        ])
        // **Closing with unsaved work asks first** (`TAURI-011`).
        //
        // Before this, the native close button and the menu's Quit discarded
        // unsaved work in silence: the editor's `beforeunload` is a *web*
        // affordance and neither route goes through it. The draft autosave
        // (`SAVE-03`) means the work is usually recoverable, but "usually
        // recoverable from somewhere you have not been told about" is not the
        // same as being asked.
        //
        // The first request is always refused, because the question cannot be
        // answered here — see `agree_to_close`. A clean document answers
        // itself and closes immediately, so the ordinary case costs one
        // round trip to the webview and no dialog.
        .on_window_event(|window, event| {
            if !matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                return;
            }
            use tauri::Manager as _;
            let shell = window.state::<Shell>();
            let agreed = shell.closing.load(std::sync::atomic::Ordering::SeqCst);
            if !casual_calc_desktop::close::should_prevent(agreed) {
                return; // already agreed: this is the close we asked for
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
            }
            // Asked in the webview because the editor owns the answer: only it
            // knows whether the document is dirty, and only it has a dialog
            // that looks like the rest of the application.
            //
            // `eval` is a `WebviewWindow` method, and this callback receives a
            // `Window` — the same distinction `on_menu_event` navigates by
            // asking the app handle for the webview by name.
            let Some(view) = window.app_handle().get_webview_window("main") else {
                return;
            };
            let _ = view.eval(casual_calc_desktop::close::confirm_close(false));
        })
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
            // The Windows and Linux route (`TAURI-010`): a double-click puts
            // the path in `argv`. Queued rather than opened — there is a window
            // here but no editor in it yet, and the webview collects when it is
            // ready. macOS never arrives this way; see the run handler below.
            if let Some(path) = launch::path_from_args(std::env::args_os()) {
                hand_over(app.handle(), path);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the application")
        // `build` + `run` rather than `Builder::run`, and the whole reason is
        // the arm below: `RunEvent` is not reachable from the short form.
        //
        // Underscore-prefixed because on Windows and Linux nothing in here uses
        // them — `RunEvent::Opened` is compiled out on those platforms, since
        // Tauri only defines the variant where the operating system sends it.
        .run(|_app, _event| {
            // **Quitting is a second route out, and it does not come through
            // the window** (`TAURI-014`). macOS Cmd+Q terminates the
            // application: `RunEvent::ExitRequested`, never
            // `WindowEvent::CloseRequested`. `TAURI-011` closed the window
            // route and deliberately left this one open rather than claiming
            // both; this is the other half, reusing the same script and the
            // same latch so there is one question and one answer, not two that
            // can disagree.
            if let tauri::RunEvent::ExitRequested { api, .. } = &_event {
                use tauri::Manager as _;
                let shell = _app.state::<Shell>();
                let agreed = shell.closing.load(std::sync::atomic::Ordering::SeqCst);
                if casual_calc_desktop::close::should_prevent(agreed) {
                    api.prevent_exit();
                    if let Some(view) = _app.get_webview_window("main") {
                        let _ = view.eval(casual_calc_desktop::close::confirm_close(true));
                    }
                }
            }
            // **macOS's only route.** Finder does not pass a path in `argv` to
            // a bundled application; it calls `application:openURLs:`, which
            // arrives here — on first launch *and* every time another file is
            // double-clicked while this window is open.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &_event {
                for url in urls {
                    // `to_file_path` rather than string surgery: it rejects the
                    // non-file URLs this event also carries, and it does the
                    // percent-decoding that turns `Q3%20figures.xlsx` back into
                    // a name the filesystem has.
                    if let Ok(path) = url.to_file_path() {
                        hand_over(_app, path);
                    }
                }
            }
        });
}
