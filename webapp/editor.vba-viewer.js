// The Visual Basic window, read-only (`MACRO-05`, docs/99 §4).
//
// Excel's **Developer ▸ Code ▸ Visual Basic** and **Alt+F11** open the Visual
// Basic Editor: a Project Explorer on the left — the project, then its folders
// "Microsoft Excel Objects", "Forms", "Modules" and "Class Modules" — and the
// selected item's Code window beside it. This is that layout, **read-only**:
// the code is shown monospace and line-numbered, with Copy, and a note says
// VBA does not run here (macros in OpenCalc are JavaScript, one click away in
// the Macros window). Present only when the workbook carries a VBA project.
//
// VBA is **never run** here. The project is decoded for reading by the engine
// (`session_vba_modules`); its bytes stay exactly as they arrived and the next
// save writes them back unchanged, whatever this window could make of them.
//
// **VBA source is untrusted input** (docs/99 §5): module names and code reach
// the page as text nodes and a textarea value only.

import { modalFrame } from "./editor.dialogs.js";
import { run as runRegistered } from "./commands.js";
import { canvas, documentName, el, errText, status, statusError, wasm } from "./editor.core.js";

/// The Project Explorer's folders, in the VBA editor's order, with the Fluent
/// icon each folder's items carry.
const FOLDERS = [
  { kind: "document", label: "Microsoft Excel Objects", icon: "table" },
  { kind: "designer", label: "Forms", icon: "textbox" },
  { kind: "standard", label: "Modules", icon: "code" },
  { kind: "class", label: "Class Modules", icon: "shapes" },
];

/// The selected module survives closing and reopening, as the Macros window's
/// selection does.
const vbaState = { selected: null };

function project() {
  let p;
  try {
    p = JSON.parse(wasm.session_vba_modules());
  } catch (e) {
    return { present: false, error: errText(e), modules: [] };
  }
  return p.present ? p : defaultProject();
}

/// What Excel's editor shows for a workbook that has never had VBA: the
/// project "VBAProject" with one document module per sheet, "Sheet1 (Sheet1)"
/// (code name, then sheet name), and ThisWorkbook, every code window empty.
/// Nothing is created in the file — the view only.
function defaultProject() {
  let names = [];
  try { names = JSON.parse(wasm.session_sheet_names()); } catch {}
  const modules = names.map((name, i) => ({ name: `Sheet${i + 1} (${name})`, kind: "document", source: "" }));
  modules.push({ name: "ThisWorkbook", kind: "document", source: "" });
  return { present: true, name: "VBAProject", modules };
}

/// The text the VBA editor shows: it hides the `Attribute VB_…` header lines
/// a module is stored with, and so does this. Line endings are VBA's CRLF.
export function visibleSource(source) {
  const lines = String(source ?? "").split(/\r\n|\r|\n/);
  let i = 0;
  while (i < lines.length && /^Attribute VB_/.test(lines[i])) i++;
  return lines.slice(i).join("\n");
}

function icon(name) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 20 20");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("fill", "currentColor");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("oc-vba-icon");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#fi-${name}`);
  svg.appendChild(use);
  return svg;
}

/// Open the Visual Basic window.
export function openVbaViewer() {
  const p = project();
  const book = documentName() || "Book1";
  // The VBA editor's own window title; that this is read-only is the note's first word.
  const frame = modalFrame(`Microsoft Visual Basic for Applications - ${book}`, {
    onCancel: () => finish(),
    onKey: (e) => {
      const active = document.activeElement;
      if ((e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End") && tree.contains(active)) {
        e.preventDefault();
        const items = [...tree.querySelectorAll(".oc-vba-item")];
        const i = items.findIndex((r) => r.classList.contains("on"));
        const to = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : i + (e.key === "ArrowDown" ? 1 : -1);
        const next = items[Math.max(0, Math.min(items.length - 1, to))];
        next?.click();
        next?.focus();
        return true;
      }
      return active === code && e.key === "Enter";
    },
  });
  const { body } = frame;

  const root = el("div", "oc-vba");
  root.id = "oc-vba";

  const note = el("div", "oc-vba-note");
  note.id = "oc-vba-note";
  note.append(document.createTextNode("Read-only. VBA does not run in OpenCalc — macros here are JavaScript: "));
  const toMacros = el("button", "oc-vba-link", "open the Macros window");
  toMacros.type = "button";
  toMacros.id = "oc-vba-to-macros";
  note.append(toMacros, document.createTextNode("."));

  const split = el("div", "oc-vba-split");

  // Project Explorer.
  const explorer = el("div", "oc-vba-explorer");
  const explorerTitle = el("div", "oc-vba-caption", `Project - ${p.name || "VBAProject"}`);
  explorerTitle.id = "oc-vba-explorer-title";
  const tree = el("div", "oc-vba-tree");
  tree.id = "oc-vba-tree";
  tree.setAttribute("role", "tree");
  tree.setAttribute("aria-labelledby", explorerTitle.id);
  explorer.append(explorerTitle, tree);

  // Code window.
  const pane = el("div", "oc-vba-pane");
  const codeTitle = el("div", "oc-vba-caption");
  codeTitle.id = "oc-vba-code-title";
  const editorBox = el("div", "oc-vba-editor");
  const gutter = el("div", "oc-vba-gutter");
  gutter.setAttribute("aria-hidden", "true");
  const code = document.createElement("textarea");
  code.id = "oc-vba-code";
  code.className = "oc-vba-code mono";
  code.readOnly = true;
  code.spellcheck = false;
  code.wrap = "off";
  code.setAttribute("aria-labelledby", codeTitle.id);
  editorBox.append(gutter, code);
  const problem = el("div", "oc-vba-problem");
  problem.id = "oc-vba-problem";
  problem.setAttribute("role", "status");
  problem.hidden = true;
  pane.append(codeTitle, editorBox, problem);
  split.append(explorer, pane);
  root.append(note, split);

  const actions = el("div", "oc-confirm-actions");
  const closeBtn = el("button", "oc-btn primary", "Close");
  closeBtn.id = "oc-vba-close";
  const copyBtn = el("button", "oc-btn", "Copy");
  copyBtn.id = "oc-vba-copy";
  actions.append(closeBtn, copyBtn);
  body.append(root, actions);

  const modules = Array.isArray(p.modules) ? p.modules : [];

  function say(text) {
    problem.textContent = text;
    problem.hidden = !text;
  }

  function numberLines() {
    const n = Math.max(1, code.value.split("\n").length);
    gutter.textContent = "";
    const frag = document.createDocumentFragment();
    for (let i = 1; i <= n; i++) frag.appendChild(el("div", null, String(i)));
    gutter.appendChild(frag);
    gutter.scrollTop = code.scrollTop;
  }

  function showSelected() {
    const m = modules.find((x) => x.name === vbaState.selected) ?? null;
    code.value = m && typeof m.source === "string" ? visibleSource(m.source) : "";
    // The VBA editor's Code window caption: "Book1.xlsm - Module1 (Code)".
    codeTitle.textContent = m ? `${book} - ${m.name} (Code)` : book;
    copyBtn.disabled = !m || typeof m.source !== "string";
    if (p.error) say(`The VBA project could not be read: ${p.error}. It is kept unchanged and is saved as it was.`);
    else if (m && m.error) say(`This module could not be read: ${m.error}`);
    else if (p.codePageSupported === false) say(`The project's code page (${p.codePage}) is not one OpenCalc reads; characters outside ASCII show as �.`);
    else say("");
    numberLines();
    code.scrollTop = 0;
  }

  function row(cls, level, iconName, text) {
    const r = el("div", cls);
    r.setAttribute("role", "treeitem");
    r.setAttribute("aria-level", String(level));
    r.style.setProperty("--level", String(level - 1));
    r.append(icon(iconName), el("span", "oc-vba-name", text));
    return r;
  }

  function render() {
    if (!modules.some((m) => m.name === vbaState.selected)) {
      // The first module in explorer order, as the tree shows them.
      const first = FOLDERS.map((f) => modules.find((m) => m.kind === f.kind)).find(Boolean);
      vbaState.selected = first?.name ?? null;
    }
    tree.textContent = "";
    if (!p.present) {
      tree.appendChild(el("div", "panel-hint oc-vba-empty", "This workbook has no VBA project."));
      showSelected();
      return;
    }
    const projectRow = row("oc-vba-project", 1, "book_open", `${p.name || "VBAProject"} (${book})`);
    projectRow.setAttribute("aria-expanded", "true");
    tree.appendChild(projectRow);
    for (const f of FOLDERS) {
      const members = modules.filter((m) => m.kind === f.kind);
      if (!members.length) continue;
      const folder = row("oc-vba-folder", 2, "folder_open", f.label);
      folder.setAttribute("aria-expanded", "true");
      tree.appendChild(folder);
      for (const m of members) {
        const item = row("oc-vba-item", 3, f.icon, m.name);
        item.tabIndex = -1;
        item.dataset.module = m.name;
        item.dataset.kind = m.kind;
        item.title = m.name;
        const on = m.name === vbaState.selected;
        item.classList.toggle("on", on);
        item.setAttribute("aria-selected", String(on));
        item.addEventListener("click", () => {
          if (m.name === vbaState.selected) return;
          vbaState.selected = m.name;
          render();
          tree.querySelector(".oc-vba-item.on")?.focus();
        });
        tree.appendChild(item);
      }
    }
    showSelected();
  }

  code.addEventListener("scroll", () => { gutter.scrollTop = code.scrollTop; });
  copyBtn.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(code.value);
      status.textContent = `copied ${vbaState.selected ?? "the module"}`;
    } catch (e) {
      // No clipboard permission: leave the code selected, so Ctrl+C copies it.
      code.focus();
      code.select();
      statusError(`Copy was refused (${errText(e)}); the code is selected — press Ctrl+C`);
    }
  });
  closeBtn.addEventListener("click", () => finish());
  toMacros.addEventListener("click", () => {
    frame.close();
    runRegistered("view.macros");
  });

  function finish() {
    frame.close();
    canvas.focus();
  }

  render();
  (tree.querySelector(".oc-vba-item.on") ?? closeBtn).focus();
}
