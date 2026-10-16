// The Macros window and the autostart offer (`MACRO-02`, docs/99 §3).
//
// **View ▸ Macros** (ONLYOFFICE's place), **Developer ▸ Macros** and **Alt+F8**
// (Excel's) open one window: the workbook's macros on the left — New, Rename,
// Delete, the Autostart flag — and an editor beside the list, with Run and an
// inline result line that names the line a failure came from.
//
// The macros live in the workbook (`casual-calc-model` `MacroLibrary`, saved as
// ONLYOFFICE's `xl/jsaProject.bin`), so every change here is an engine edit —
// one undo step, refused by the engine in a read-only session, sent to
// collaborators as an operation. Typing in the editor is not a change: the
// source is committed when it is run, when another macro is selected, and when
// the window closes, so a paragraph of typing is one step and not hundreds.
//
// **Script source is untrusted input everywhere it is displayed** (docs/99 §5):
// names and code reach the page as text nodes and textarea values only.
//
// The autostart offer: a workbook whose macros ask to run on open gets a
// non-modal message bar — Run / Not now — and nothing runs until the person
// chooses Run. Never offered read-only, never where the mode withholds
// `canRunMacros` (embedded and WOPI by default), and the choice is remembered
// for this document for the life of the tab only.

import { modalFrame, confirmModal } from "./editor.dialogs.js";
import { runMacro } from "./editor.macros.js";
import { customFunctionErrors, customFunctionInfo, customFunctionsSettledForTest, trustCustomFunctions } from "./editor.custom-functions.js";
import {
  byId,
  canvas,
  el,
  errText,
  getCapabilities,
  status,
  statusError,
  wasm,
} from "./editor.core.js";

/// What ONLYOFFICE's Macros window puts in a new macro, so a macro started
/// here looks like one started there.
const NEW_MACRO = "(function()\n{\n    // Enter your code here.\n})();\n";
/// A new custom function (`MACRO-04`): ONLYOFFICE's shape — a JSDoc block with
/// the required `@customfunction` tag, then `Api.AddCustomFunction`.
const NEW_FUNCTION = "(function()\n{\n    /**\n     * Function that returns the argument\n     * @customfunction\n     * @param {any} arg Any data.\n     * @returns {any} The argument of the function.\n     */\n    function myFunction(arg) {\n        return arg;\n    }\n    Api.AddCustomFunction(myFunction);\n})();\n";
const INDENT = "    ";

/// Selection and scroll survive closing and reopening, as the Name Manager's do
/// — which is what lets Delete close the window for its confirmation and come
/// back to the same place. Excel asks over the dialog instead; that is a
/// difference still to convert (XL-UX-169's audit), not a rule.
const mwState = { selected: null };

function library() {
  try {
    const lib = JSON.parse(wasm.session_macros());
    return Array.isArray(lib.macros) ? lib.macros : [];
  } catch {
    return [];
  }
}

/// Macros, then custom-function sources (`MACRO-04`), as the window lists
/// them — ONLYOFFICE's Macros window keeps the two under their own headings.
function entries() {
  let lib = { macros: [], customFunctions: [] };
  try { lib = JSON.parse(wasm.session_macros()); } catch {}
  return [
    ...(lib.macros || []).map((m) => ({ ...m, kind: "macro" })),
    ...(lib.customFunctions || []).map((f) => ({ ...f, autostart: false, kind: "function" })),
  ];
}

function newGuid() {
  try { if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID(); } catch {}
  let s = "";
  for (let i = 0; i < 32; i++) s += Math.floor(Math.random() * 16).toString(16);
  return s;
}

/// "Macro 1", "Macro 2", … — ONLYOFFICE's naming, first number not taken;
/// "Function 1", … for custom functions.
function nextName(macros, word = "Macro") {
  const taken = new Set(macros.map((m) => m.name.toLowerCase()));
  for (let n = 1; ; n++) if (!taken.has(`${word.toLowerCase()} ${n}`)) return `${word} ${n}`;
}

function editable() {
  let readOnly = false;
  try { readOnly = !!wasm.session_read_only(); } catch {}
  return !readOnly && !getCapabilities().readOnly;
}

/// Open the Macros window.
export function openMacrosWindow() {
  const canEdit = editable();
  let renaming = null;
  let dirty = false;

  const frame = modalFrame("Macros", {
    onCancel: () => finish(),
    onKey: (e) => {
      const active = document.activeElement;
      if (renaming && active === renaming.input) {
        if (e.key === "Escape") { e.stopPropagation(); e.preventDefault(); endRename(false); return true; }
        if (e.key === "Enter") { e.stopPropagation(); e.preventDefault(); endRename(true); return true; }
        return true;
      }
      if (active === code) {
        // The editor's own keys: Enter is a newline, Tab indents.
        if (e.key === "Enter") return true;
        if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && canEdit) {
          e.preventDefault();
          e.stopPropagation();
          indent();
          return true;
        }
        return false;
      }
      if ((e.key === "ArrowDown" || e.key === "ArrowUp") && list.contains(active)) {
        e.preventDefault();
        const rows = [...list.querySelectorAll(".oc-mw-row")];
        const i = rows.findIndex((r) => r.classList.contains("on"));
        const next = rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))];
        next?.click();
        next?.focus();
        return true;
      }
      if (e.key === "Delete" && list.contains(active) && canEdit) {
        e.preventDefault();
        del.click();
        return true;
      }
      return false;
    },
  });
  const { body } = frame;

  const root = el("div", "oc-mw");
  root.id = "oc-mw";
  const bar = el("div", "oc-mw-bar");
  const neu = el("button", "oc-btn", "New");
  neu.id = "oc-mw-new";
  const neuFn = el("button", "oc-btn", "New function");
  neuFn.id = "oc-mw-new-function";
  const ren = el("button", "oc-btn", "Rename");
  ren.id = "oc-mw-rename";
  const del = el("button", "oc-btn", "Delete");
  del.id = "oc-mw-delete";
  if (canEdit) bar.append(neu, neuFn, ren, del);

  const split = el("div", "oc-mw-split");
  const list = el("div", "oc-mw-list");
  list.id = "oc-mw-list";
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", "Macros");

  const pane = el("div", "oc-mw-pane");
  const editorBox = el("div", "oc-mw-editor");
  const gutter = el("div", "oc-mw-gutter");
  gutter.setAttribute("aria-hidden", "true");
  const code = document.createElement("textarea");
  code.id = "oc-mw-code";
  code.className = "oc-mw-code mono";
  code.spellcheck = false;
  code.autocomplete = "off";
  code.setAttribute("autocapitalize", "off");
  code.setAttribute("aria-label", "Macro code");
  code.wrap = "off";
  code.readOnly = !canEdit;
  editorBox.append(gutter, code);

  const autoRow = el("label", "oc-mw-auto");
  const auto = document.createElement("input");
  auto.type = "checkbox";
  auto.id = "oc-mw-autostart";
  autoRow.append(auto, document.createTextNode(" Autostart — offer to run this macro when the workbook opens"));
  if (!canEdit) autoRow.hidden = true;

  const result = el("div", "oc-mw-result");
  result.id = "oc-mw-result";
  result.setAttribute("role", "status");
  result.hidden = true;

  pane.append(editorBox, autoRow, result);
  split.append(list, pane);
  root.append(bar, split);

  const actions = el("div", "oc-confirm-actions");
  const runBtn = el("button", "oc-btn primary", "Run");
  runBtn.id = "oc-mw-run";
  const closeBtn = el("button", canEdit ? "oc-btn" : "oc-btn primary", "Close");
  closeBtn.id = "oc-mw-close";
  if (canEdit) actions.append(runBtn);
  actions.append(closeBtn);
  body.append(root, actions);

  let macros = [];
  const selected = () => macros.find((m) => m.guid === mwState.selected) ?? null;

  /// What a custom-function source registered, or why it did not.
  function functionReport(m) {
    const errors = customFunctionErrors().filter((e) => !e.guid || e.guid === m.guid);
    if (errors.length) {
      const e = errors[0];
      return { text: e.line ? `Line ${e.line}: ${e.message}` : e.message, bad: true, line: Number(e.line) || 0 };
    }
    const names = customFunctionInfo().map((f) => f.name);
    return { text: names.length ? `Registered: ${names.join(", ")}. Use them in a formula, as =${names[0]}(…).` : "No functions registered yet.", bad: false, line: 0 };
  }

  function numberLines(errorLine = 0) {
    const n = Math.max(1, code.value.split("\n").length);
    gutter.textContent = "";
    for (let i = 1; i <= n; i++) {
      const d = el("div", i === errorLine ? "bad" : null, String(i));
      gutter.appendChild(d);
    }
    gutter.scrollTop = code.scrollTop;
  }

  function showSelected() {
    const m = selected();
    code.value = m ? m.source : "";
    code.disabled = !m;
    auto.checked = !!m?.autostart;
    auto.disabled = !m;
    // A custom function is not run; it is saved, and the sandbox registers it.
    const isFn = m?.kind === "function";
    autoRow.hidden = !canEdit || isFn;
    runBtn.textContent = isFn ? "Save" : "Run";
    code.setAttribute("aria-label", isFn ? "Custom function code" : "Macro code");
    runBtn.disabled = !m;
    ren.disabled = !m;
    del.disabled = !m;
    dirty = false;
    numberLines();
  }

  function render() {
    macros = entries();
    if (!macros.some((m) => m.guid === mwState.selected)) mwState.selected = macros[0]?.guid ?? null;
    list.textContent = "";
    const heading = (text) => {
      const h = el("div", "oc-mw-heading", text);
      h.setAttribute("role", "presentation");
      list.appendChild(h);
    };
    const anyMacros = macros.some((m) => m.kind === "macro");
    const anyFns = macros.some((m) => m.kind === "function");
    if (anyFns) heading("Macros");
    let fnHeading = false;
    for (const m of macros) {
      if (m.kind === "function" && !fnHeading) {
        if (!anyMacros) list.appendChild(el("div", "panel-hint oc-mw-empty", "No macros."));
        heading("Custom functions");
        fnHeading = true;
      }
      const row = el("div", "oc-mw-row");
      row.setAttribute("role", "option");
      row.tabIndex = -1;
      row.dataset.guid = m.guid;
      row.dataset.kind = m.kind;
      // Text nodes: a macro name is workbook text.
      const name = el("span", "oc-mw-name", m.name);
      name.title = m.name;
      row.appendChild(name);
      if (m.autostart) {
        const tag = el("span", "oc-mw-tag", "Autostart");
        row.appendChild(tag);
      }
      const on = m.guid === mwState.selected;
      row.classList.toggle("on", on);
      row.setAttribute("aria-selected", String(on));
      row.addEventListener("click", () => {
        if (renaming) endRename(true);
        if (m.guid === mwState.selected) return;
        commitSource();
        mwState.selected = m.guid;
        render();
        result.hidden = true;
      });
      list.appendChild(row);
    }
    if (!macros.length) {
      list.appendChild(el("div", "panel-hint oc-mw-empty", canEdit ? "No macros yet. Choose New to write one." : "This workbook has no macros."));
    }
    showSelected();
  }

  /// Put the editor's text into the workbook — one edit, and only when it
  /// changed. Returns false (with the reason shown inline) when refused.
  function commitSource() {
    const m = selected();
    if (!m || !dirty || !canEdit) return true;
    try {
      wasm.session_set_macro_source(m.guid, code.value);
      m.source = code.value;
      dirty = false;
      if (m.kind === "function") trustCustomFunctions();
      return true;
    } catch (e) {
      say(`Not saved: ${errText(e)}`, true);
      return false;
    }
  }

  function say(text, bad = false, line = 0) {
    result.textContent = text;
    result.classList.toggle("bad", bad);
    result.hidden = false;
    numberLines(line);
  }

  function indent() {
    const { selectionStart: a, selectionEnd: b, value } = code;
    code.value = value.slice(0, a) + INDENT + value.slice(b);
    code.selectionStart = code.selectionEnd = a + INDENT.length;
    dirty = true;
    numberLines();
  }

  function startRename() {
    const m = selected();
    const row = m && list.querySelector(`.oc-mw-row[data-guid="${CSS.escape(m.guid)}"]`);
    if (!row) return;
    const input = document.createElement("input");
    input.type = "text";
    input.id = "oc-mw-rename-input";
    input.className = "oc-mw-rename-input";
    input.value = m.name;
    input.setAttribute("aria-label", "Macro name");
    row.textContent = "";
    row.appendChild(input);
    renaming = { input, guid: m.guid };
    input.addEventListener("blur", () => { if (renaming?.input === input) endRename(true); });
    input.focus();
    input.select();
  }

  function endRename(keep) {
    const r = renaming;
    if (!r) return;
    renaming = null;
    const name = r.input.value.trim();
    if (keep && name) {
      try { wasm.session_rename_macro(r.guid, name); } catch (e) { say(`Not renamed: ${errText(e)}`, true); }
    } else if (keep && !name) {
      say("A macro needs a name.", true);
    }
    render();
    list.querySelector(".oc-mw-row.on")?.focus();
  }

  code.addEventListener("input", () => { dirty = true; numberLines(); });
  code.addEventListener("scroll", () => { gutter.scrollTop = code.scrollTop; });
  code.addEventListener("blur", () => commitSource());

  auto.addEventListener("change", () => {
    const m = selected();
    if (!m) return;
    commitSource();
    try { wasm.session_set_macro_autostart(m.guid, auto.checked); } catch (e) { say(errText(e), true); }
    render();
  });

  neuFn.addEventListener("click", () => {
    if (renaming) endRename(true);
    commitSource();
    const guid = newGuid();
    try {
      wasm.session_set_custom_function_source(guid, nextName(macros.filter((m) => m.kind === "function"), "Function"), NEW_FUNCTION);
    } catch (e) {
      say(`Not created: ${errText(e)}`, true);
      return;
    }
    // Written here, by the person: this workbook's functions may run.
    trustCustomFunctions();
    mwState.selected = guid;
    render();
    result.hidden = true;
    code.focus();
  });

  neu.addEventListener("click", () => {
    if (renaming) endRename(true);
    commitSource();
    const guid = newGuid();
    try {
      wasm.session_set_macro(guid, nextName(macros), NEW_MACRO, false);
    } catch (e) {
      say(`Not created: ${errText(e)}`, true);
      return;
    }
    mwState.selected = guid;
    render();
    result.hidden = true;
    code.focus();
    // Inside the function body, where ONLYOFFICE puts the cursor.
    const at = code.value.indexOf("//");
    code.selectionStart = code.selectionEnd = at >= 0 ? at : code.value.length;
  });

  ren.addEventListener("click", () => { commitSource(); startRename(); });

  del.addEventListener("click", () => {
    const m = selected();
    if (!m) return;
    commitSource();
    // Closed for the question and reopened after it, the Name Manager's
    // pattern. Excel asks over the dialog; still to convert (XL-UX-169).
    frame.close();
    const what = m.kind === "function" ? "custom function source" : "macro";
    confirmModal("Macros", `Delete the ${what} "${m.name}"? This can be undone with Undo.`, "Delete").then((yes) => {
      if (yes) {
        try { wasm.session_delete_macro(m.guid); mwState.selected = null; }
        catch (e) { statusError(errText(e)); }
      }
      openMacrosWindow();
    });
  });

  runBtn.addEventListener("click", async () => {
    const m = selected();
    if (!m) return;
    if (!commitSource()) return;
    if (m.kind === "function") {
      runBtn.disabled = true;
      say(`Registering ${m.name}…`);
      trustCustomFunctions();
      try { await customFunctionsSettledForTest(); } finally { runBtn.disabled = false; }
      if (!root.isConnected) return;
      const r = functionReport(selected() ?? m);
      say(r.text, r.bad, r.line);
      if (r.line) selectLine(r.line);
      return;
    }
    runBtn.disabled = true;
    say(`Running ${m.name}…`);
    let res;
    try {
      res = await runMacro(m.source, { name: m.name, guid: m.guid });
    } finally {
      runBtn.disabled = false;
    }
    if (!root.isConnected) return;
    if (res.ok) {
      const n = res.operations.length;
      say(n ? `Ran ${m.name}: ${n} change${n === 1 ? "" : "s"}, one undo step.` : `Ran ${m.name}: nothing changed.`);
    } else {
      const line = Number(res.error?.line) || 0;
      say(line ? `Line ${line}: ${res.error.message}` : res.error?.message || "The macro failed.", true, line);
      if (line) selectLine(line);
    }
  });

  function selectLine(line) {
    const lines = code.value.split("\n");
    let start = 0;
    for (let i = 0; i < line - 1 && i < lines.length; i++) start += lines[i].length + 1;
    const end = start + (lines[line - 1] ?? "").length;
    code.focus();
    code.setSelectionRange(start, end);
  }

  const finish = () => {
    if (renaming) endRename(true);
    commitSource();
    frame.close();
    canvas.focus();
  };
  closeBtn.addEventListener("click", finish);

  render();
  (list.querySelector(".oc-mw-row.on") ?? (canEdit ? neu : closeBtn)).focus();
}

// ---- Autostart, safely (docs/99 §3 "Trust") ---------------------------------

/// This tab's answers, per document: `"run"` or `"later"`. Keyed by the
/// document's name **and** its autostart macros, so a different file that
/// happens to share a name — or this one with its macros changed — is asked
/// again rather than trusted on the strength of the old answer.
const answers = new Map();

function autostartKey(name) {
  const autos = library().filter((m) => m.autostart).map((m) => [m.guid, m.name, m.source]);
  return `${name || ""}\u0000${JSON.stringify(autos)}`;
}

function bar() { return byId("oc-macro-bar"); }

function hideBar() {
  const b = bar();
  if (b) { b.hidden = true; b.replaceChildren(); }
}

/// Whether this editor may offer to run a workbook's macros at all.
function mayRunMacros() {
  const caps = getCapabilities();
  if (caps.readOnly || caps.canRunMacros === false) return false;
  let readOnly = false;
  try { readOnly = !!wasm.session_read_only(); } catch {}
  return !readOnly;
}

async function runAutostart() {
  const autos = library().filter((m) => m.autostart);
  let ran = 0;
  for (const m of autos) {
    const res = await runMacro(m.source, { name: m.name, guid: m.guid });
    if (!res.ok) {
      const line = Number(res.error?.line) || 0;
      statusError(`autostart macro "${m.name}" failed${line ? ` at line ${line}` : ""}: ${res.error?.message ?? "error"}`);
      return;
    }
    ran++;
  }
  status.textContent = `ran ${ran} autostart macro${ran === 1 ? "" : "s"}`;
}

/// Called after a workbook opens. Offers — never runs — its autostart macros.
export function offerAutostartMacros(name) {
  hideBar();
  let has = false;
  try { has = !!wasm.session_has_autostart_macros(); } catch {}
  if (!has || !mayRunMacros()) return;
  const key = autostartKey(name);
  const answered = answers.get(key);
  if (answered === "later") return;
  if (answered === "run") {
    // Agreed to earlier in this tab, for exactly these macros.
    runAutostart();
    return;
  }
  const b = bar();
  if (!b) return;
  const row = el("div", "oc-recovery-row");
  row.appendChild(el("strong", null, "Macros"));
  row.appendChild(el("span", "oc-recovery-detail", " — This workbook has macros set to run when it opens. Run them?"));
  const actions = el("span", "oc-recovery-actions");
  const runIt = el("button", "oc-btn primary", "Run");
  runIt.id = "oc-macro-bar-run";
  const later = el("button", "oc-btn", "Not now");
  later.id = "oc-macro-bar-later";
  actions.append(runIt, later);
  row.appendChild(actions);
  b.replaceChildren(row);
  b.hidden = false;
  runIt.addEventListener("click", () => {
    answers.set(key, "run");
    hideBar();
    runAutostart();
  });
  later.addEventListener("click", () => {
    answers.set(key, "later");
    hideBar();
    canvas.focus();
  });
}

/// Test hook: forget this tab's answers, as a new tab would.
export function forgetMacroAnswersForTest() {
  answers.clear();
}
