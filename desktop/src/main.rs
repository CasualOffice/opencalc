//! The desktop application: a window, and an operating-system menu.
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

// The window is the point of this binary; there is nothing to print.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use casual_calc_desktop::menu::{self, Menu as MenuModel, Node};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager, WebviewWindow};

/// Build the platform menu from the editor's own model.
fn build_menu(app: &tauri::AppHandle, model: &[MenuModel]) -> tauri::Result<Menu<tauri::Wry>> {
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

fn append_nodes(
    app: &tauri::AppHandle,
    into: &Submenu<tauri::Wry>,
    nodes: &[Node],
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![publish_menu])
        .on_menu_event(|app, event| {
            // The id is the editor's own command id, so this is the whole of
            // the dispatch: no second table, no mapping to keep in step.
            let id = event.id().0.replace('\\', "\\\\").replace('\'', "\\'");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval(&format!(
                    "try {{ window.opencalcEditor.runCommand('{id}') }} \
                     catch (e) {{ console.error('[opencalc] menu', e) }}"
                ));
            }
        })
        .setup(|app| {
            let window = app.get_webview_window("main").expect("the main window");
            // The editor builds its menus during boot, so the model is asked
            // for after the page settles rather than at window creation.
            let _ = window.eval(
                "(function wait() {\
                   if (window.opencalcEditor && window.opencalcEditor.menuModel) {\
                     window.__TAURI__.core.invoke('publish_menu', \
                       { model: JSON.stringify(window.opencalcEditor.menuModel()) });\
                   } else { setTimeout(wait, 60); }\
                 })()",
            );
            let _ = app.emit("ready", ());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the application");
}
