// Find and Replace, and resolving a typed reference to a place to go.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding.
//
// Find and Replace is Excel's dialog (fit-gap XL-UX-024/025): Find and Replace
// tabs, Options >> for Within / Search / Look in / Match case / Match entire
// cell contents, Find All with its results list, and Find Next walking from the
// active cell. It is **modeless**, as Excel's is — the grid underneath keeps
// taking clicks and keys — and what it has to say about the search is said in
// Excel's message box over it (docs/95 rule 8, XL-UX-168). The search itself is the
// engine's `session_find_ex`; Replace All is `session_replace_all_ex`, one
// undo step. There is no second matcher here.

import {
  A1,
  byId,
  canvas,
  draw,
  effectiveRange,
  el,
  ensureVisible,
  errText,
  findState,
  messageBox,
  selectBoxes,
  sheetRef,
  invalidateGrowth,
  navBlock,
  select,
  state,
  status,
  statusError,
  switchSheet,
  updateNameBox,
  usedBounds,
  wasm,
} from "./editor.core.js";
import { syncVolatileClock } from "./editor.selection.js";

export function colName(n) {
  let s = "";
  n += 1;
  while (n > 0) {
    n -= 1;
    s = String.fromCharCode(65 + (n % 26)) + s;
    n = Math.floor(n / 26);
  }
  return s;
}

export function navTarget() {
  if (navBlock) return navBlock;
  const r = effectiveRange();
  const multi = r.r1 > r.r0 || r.c1 > r.c0;
  return multi ? r : null;
}

// ---- Find and Replace (XL-UX-024/025) ----------------------------------------

/// Excel's words, verbatim. NOT_FOUND as Microsoft Q&A quotes it
/// (learn.microsoft.com/en-us/answers/questions/5173659); it had lost its
/// second sentence here.
const NOT_FOUND = "We couldn't find what you were looking for. Click Options for more ways to search.";
const NOTHING_TO_REPLACE = "We couldn't find anything to replace. Click Options for more ways to search.";

/// Find All draws at most this many rows; the count above it is always the
/// true total. A list of a million DOM rows would hang the tab it is in.
const RESULT_ROWS = 1000;

/// What the dialog remembers between openings, as Excel's does for the session.
const fr = {
  what: "",
  with: "",
  within: "sheet",
  search: "rows",
  lookIn: "formulas",
  matchCase: false,
  whole: false,
  options: false,
  tab: "find",
  /// The open dialog's root inside `#oc-modal-body`, or null.
  root: null,
  /// How to put the dialog's controls back in step with `fr`, while open.
  sync: null,
  /// Tear the open dialog down.
  teardown: null,
};

/// The engine options for the current settings. Replace reads formulas only,
/// as Excel's Replace tab does.
function engineOpts(forReplace) {
  return JSON.stringify({
    matchCase: fr.matchCase,
    wholeCell: fr.whole,
    lookIn: forReplace ? "formulas" : fr.lookIn,
    allSheets: fr.within === "workbook",
    byColumns: fr.search === "columns",
  });
}

function search(forReplace = false) {
  if (!fr.what) return [];
  try { return JSON.parse(wasm.session_find_ex(state.sheet, fr.what, engineOpts(forReplace))); }
  catch { return []; }
}

/// Where a cell sits in the order Find Next walks: sheet, then row-major or
/// column-major. The engine returns hits in this order already.
function orderKey(s, r, c) {
  return fr.search === "columns" ? [s, c, r] : [s, r, c];
}
function cmpKey(a, b) {
  for (let i = 0; i < 3; i += 1) if (a[i] !== b[i]) return a[i] - b[i];
  return 0;
}

function goTo(m) {
  // A workbook-wide hit can be on another sheet: follow it there.
  if (m.s !== state.sheet) switchSheet(m.s);
  select(m.r, m.c);
  updateNameBox();
}

/// Excel's message box over the dialog (or over the grid, for Shift+F4 with
/// the dialog closed). OK returns to Find what with its text selected.
function say(text, icon = "warning") {
  if (!text) return;
  const what = fr.root?.isConnected ? fr.root.querySelector("#oc-fr-what") : null;
  messageBox(text, { icon, field: what });
}

/// Find Next (dir 1) or Find Previous (dir -1) from the active cell, wrapping.
/// Returns whether anything was found.
export function findStep(dir = 1) {
  const matches = search();
  findState.matches = matches;
  if (!matches.length) {
    findState.idx = -1;
    say(fr.what ? NOT_FOUND : "");
    draw();
    return false;
  }
  const here = orderKey(state.sheet, state.sel.row, state.sel.col);
  let idx;
  if (dir > 0) {
    idx = matches.findIndex((m) => cmpKey(orderKey(m.s, m.r, m.c), here) > 0);
    if (idx < 0) idx = 0;
  } else {
    idx = -1;
    for (let i = matches.length - 1; i >= 0; i -= 1) {
      if (cmpKey(orderKey(matches[i].s, matches[i].r, matches[i].c), here) < 0) { idx = i; break; }
    }
    if (idx < 0) idx = matches.length - 1;
  }
  findState.idx = idx;
  goTo(matches[idx]);
  return true;
}

/// The single-cell names in the workbook, keyed `sheet-index:row:col`, for
/// Find All's Name column.
function cellNames() {
  const out = new Map();
  let names = [], sheets = [];
  try { names = JSON.parse(wasm.session_names()); sheets = JSON.parse(wasm.session_sheet_names()); } catch { return out; }
  for (const n of names) {
    const m = /^=?(?:'((?:[^']|'')+)'|([^!]+))!\$?([A-Za-z]{1,3})\$?(\d+)$/.exec(n.refersTo || "");
    if (!m) continue;
    const sheetName = (m[1] ?? m[2]).replace(/''/g, "'");
    const s = sheets.findIndex((x) => x.toLowerCase() === sheetName.toLowerCase());
    const c = colFromName(m[3]);
    if (s < 0 || c === null) continue;
    const key = `${s}:${parseInt(m[4], 10) - 1}:${c}`;
    out.set(key, out.has(key) ? `${out.get(key)}, ${n.name}` : n.name);
  }
  return out;
}

function findAll() {
  const root = fr.root;
  if (!root) return;
  const matches = search();
  findState.matches = matches;
  findState.idx = -1;
  const wrap = root.querySelector(".oc-fr-results-wrap");
  const tbody = root.querySelector("#oc-fr-results tbody");
  const count = root.querySelector("#oc-fr-count");
  tbody.textContent = "";
  if (!matches.length) {
    wrap.hidden = true;
    count.textContent = "";
    say(fr.what ? NOT_FOUND : "");
    draw();
    return;
  }
  let sheets = [];
  try { sheets = JSON.parse(wasm.session_sheet_names()); } catch {}
  const book = byId("doc-name")?.textContent.trim() || "";
  const names = cellNames();
  for (const [i, m] of matches.slice(0, RESULT_ROWS).entries()) {
    const tr = document.createElement("tr");
    tr.className = "oc-fr-row";
    tr.tabIndex = -1;
    const cells = [book, sheets[m.s] ?? "", names.get(`${m.s}:${m.r}:${m.c}`) ?? "",
      `$${colName(m.c)}$${m.r + 1}`, m.v, m.f.startsWith("=") ? m.f : ""];
    for (const text of cells) {
      const td = document.createElement("td");
      td.textContent = text;
      td.title = text;
      tr.appendChild(td);
    }
    tr.addEventListener("click", () => {
      for (const o of tbody.querySelectorAll("tr.on")) o.classList.remove("on");
      tr.classList.add("on");
      findState.idx = i;
      goTo(m);
    });
    tbody.appendChild(tr);
  }
  wrap.hidden = false;
  count.textContent = `${matches.length} cell(s) found`;
  draw();
}

/// Run an edit the way `tryEdit` does, but answer a refusal in the dialog.
function edit(fn) {
  syncVolatileClock();
  let out;
  try { out = fn(); }
  catch (e) { say(errText(e)); out = undefined; }
  invalidateGrowth();
  draw();
  return out;
}

function replaceOne() {
  if (!fr.what) return;
  const matches = search(true);
  const here = matches.find((m) => m.s === state.sheet && m.r === state.sel.row && m.c === state.sel.col);
  if (here) {
    const did = edit(() => wasm.session_replace_at_ex(state.sheet, here.r, here.c, fr.what, fr.with, engineOpts(true)));
    if (did === undefined) return;
  }
  // Excel's Replace: rewrite the active cell if it matches, then go on.
  findStep(1);
}

function replaceAll() {
  if (!fr.what) return;
  const n = edit(() => wasm.session_replace_all_ex(state.sheet, fr.what, fr.with, engineOpts(true)));
  if (n === undefined) return;
  findState.matches = [];
  if (n > 0) say(`All done. We made ${n} replacement${n === 1 ? "" : "s"}.`, "info");
  else say(NOTHING_TO_REPLACE);
  const wrap = fr.root?.querySelector(".oc-fr-results-wrap");
  if (wrap) wrap.hidden = true;
}

/// Open the dialog on `tab`, or switch an open one to it.
function openDialog(tab) {
  if (fr.root && fr.root.isConnected && !byId("oc-modal").hidden) {
    fr.tab = tab;
    fr.sync();
    focusFirst();
    return;
  }
  fr.tab = tab;
  buildDialog();
  focusFirst();
}

function focusFirst() {
  // Ctrl+H lands the caret in Replace with — the contract UX-KEY-02 set and
  // editor.excel-shortcuts.spec.mjs pins; Ctrl+F lands it in Find what.
  const target = fr.tab === "replace" ? "#oc-fr-with" : "#oc-fr-what";
  const f = fr.root.querySelector(target);
  f.focus();
  f.select();
}

export function openFind() { openDialog("find"); }

// Excel's Ctrl+H: the same dialog on its Replace tab.
export function openReplace() { openDialog("replace"); }

export function closeFind() {
  if (fr.teardown) fr.teardown(true);
}

function buildDialog() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  const box = modal.querySelector(".oc-modal-box");
  const title = byId("oc-modal-title");
  const x = byId("oc-modal-x");
  title.textContent = "Find and Replace";
  body.textContent = "";

  const root = el("div", "oc-fr");
  fr.root = root;

  // Tabs.
  const tabs = el("div", "fc-tabs oc-fr-tabs");
  tabs.setAttribute("role", "tablist");
  const tabBtn = (id, label) => {
    const b = el("button", "fc-tab", label);
    b.type = "button";
    b.id = `oc-fr-tab-${id}`;
    b.setAttribute("role", "tab");
    b.addEventListener("click", () => { fr.tab = id; sync(); focusFirst(); });
    tabs.appendChild(b);
    return b;
  };
  const findTab = tabBtn("find", "Find");
  const replaceTab = tabBtn("replace", "Replace");

  // Fields.
  const field = (id, label) => {
    const l = el("label", "oc-form-label", label);
    l.htmlFor = id;
    const i = document.createElement("input");
    i.type = "text";
    i.id = id;
    i.className = "oc-form-field";
    i.spellcheck = false;
    i.autocomplete = "off";
    return [l, i];
  };
  const [whatLabel, what] = field("oc-fr-what", "Find what:");
  const [withLabel, withInput] = field("oc-fr-with", "Replace with:");
  what.value = fr.what;
  withInput.value = fr.with;
  what.addEventListener("input", () => { fr.what = what.value; });
  withInput.addEventListener("input", () => { fr.with = withInput.value; });
  const fields = el("div", "oc-fr-fields");
  fields.append(whatLabel, what, withLabel, withInput);

  // Options.
  const opts = el("div", "oc-fr-options");
  opts.id = "oc-fr-options";
  const choice = (id, label, items, key) => {
    const l = el("label", "oc-form-label", label);
    l.htmlFor = id;
    const s = document.createElement("select");
    s.id = id;
    s.className = "oc-form-field";
    s.dataset.items = JSON.stringify(items);
    s.addEventListener("change", () => { fr[key] = s.value; });
    return [l, s];
  };
  const [withinL, within] = choice("oc-fr-within", "Within:", [["sheet", "Sheet"], ["workbook", "Workbook"]], "within");
  const [searchL, searchSel] = choice("oc-fr-search", "Search:", [["rows", "By Rows"], ["columns", "By Columns"]], "search");
  const [lookL, lookIn] = choice("oc-fr-lookin", "Look in:", [], "lookIn");
  const check = (id, label, key) => {
    const l = el("label", "oc-check");
    const c = document.createElement("input");
    c.type = "checkbox";
    c.id = id;
    c.checked = fr[key];
    c.addEventListener("change", () => { fr[key] = c.checked; });
    l.append(c, document.createTextNode(label));
    return l;
  };
  const selects = el("div", "oc-fr-selects");
  selects.append(withinL, within, searchL, searchSel, lookL, lookIn);
  const checks = el("div", "oc-fr-checks");
  checks.append(check("oc-fr-case", "Match case", "matchCase"), check("oc-fr-whole", "Match entire cell contents", "whole"));
  opts.append(selects, checks);

  const optBtn = el("button", "oc-btn oc-fr-options-btn", "");
  optBtn.type = "button";
  optBtn.setAttribute("aria-controls", "oc-fr-options");
  optBtn.addEventListener("click", () => { fr.options = !fr.options; sync(); });
  const optRow = el("div", "oc-fr-options-row");
  optRow.append(optBtn);


  // Footer, in Excel's order. No button is primary: Find Next is the default
  // (Enter), and Excel draws none of them filled.
  const actions = el("div", "oc-confirm-actions oc-fr-actions");
  const button = (label, run) => {
    const b = el("button", "oc-btn", label);
    b.type = "button";
    b.addEventListener("click", run);
    actions.appendChild(b);
    return b;
  };
  const replaceAllBtn = button("Replace All", replaceAll);
  const replaceBtn = button("Replace", replaceOne);
  button("Find All", findAll);
  button("Find Next", (e) => findStep(e.shiftKey ? -1 : 1));
  button("Close", () => close(true));

  // Find All's results.
  const resultsWrap = el("div", "oc-fr-results-wrap");
  resultsWrap.hidden = true;
  const scroller = el("div", "oc-fr-scroll");
  const table = document.createElement("table");
  table.id = "oc-fr-results";
  table.className = "oc-fr-table";
  const thead = document.createElement("thead");
  const hr = document.createElement("tr");
  for (const h of ["Book", "Sheet", "Name", "Cell", "Value", "Formula"]) {
    const th = document.createElement("th");
    th.textContent = h;
    hr.appendChild(th);
  }
  thead.appendChild(hr);
  table.append(thead, document.createElement("tbody"));
  scroller.appendChild(table);
  const count = el("div", "oc-fr-count");
  count.id = "oc-fr-count";
  count.setAttribute("role", "status");
  resultsWrap.append(scroller, count);

  root.append(tabs, fields, opts, optRow, actions, resultsWrap);
  body.appendChild(root);

  function sync() {
    const rep = fr.tab === "replace";
    findTab.classList.toggle("on", !rep);
    replaceTab.classList.toggle("on", rep);
    findTab.setAttribute("aria-selected", String(!rep));
    replaceTab.setAttribute("aria-selected", String(rep));
    withLabel.hidden = !rep;
    withInput.hidden = !rep;
    replaceAllBtn.hidden = !rep;
    replaceBtn.hidden = !rep;
    // Look in: Replace reads formulas only.
    const lookItems = rep ? [["formulas", "Formulas"]] : [["formulas", "Formulas"], ["values", "Values"], ["notes", "Notes"]];
    lookIn.dataset.items = JSON.stringify(lookItems);
    if (rep) fr.lookIn = "formulas";
    for (const [s, key] of [[within, "within"], [searchSel, "search"], [lookIn, "lookIn"]]) {
      s.textContent = "";
      for (const [v, label] of JSON.parse(s.dataset.items)) {
        const o = document.createElement("option");
        o.value = v;
        o.textContent = label;
        s.appendChild(o);
      }
      s.value = fr[key];
    }
    opts.hidden = !fr.options;
    optBtn.textContent = fr.options ? "Options <<" : "Options >>";
    optBtn.setAttribute("aria-expanded", String(fr.options));
  }
  fr.sync = sync;
  sync();

  // Modeless: no backdrop, the grid stays live, and the box can be dragged by
  // its title. The shared frame is reused so the dialog looks like every other
  // one; the class comes off again when anything else takes the frame.
  modal.classList.add("oc-modeless");
  box.setAttribute("aria-modal", "false");
  box.style.left = fr.left ?? "";
  box.style.top = fr.top ?? "";
  modal.hidden = false;
  findState.open = true;

  const drag = (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const x0 = e.clientX - (parseFloat(box.style.left) || 0);
    const y0 = e.clientY - (parseFloat(box.style.top) || 0);
    const move = (ev) => {
      box.style.left = `${ev.clientX - x0}px`;
      box.style.top = `${ev.clientY - y0}px`;
    };
    const up = () => {
      fr.left = box.style.left;
      fr.top = box.style.top;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  title.addEventListener("pointerdown", drag);
  title.classList.add("oc-drag-handle");

  // Keys reach the dialog only while focus is inside it; on the grid they
  // belong to the grid, as with Excel's modeless dialog.
  const inside = (e) => root.contains(e.composedPath ? e.composedPath()[0] : e.target);
  const onKey = (e) => {
    if (!root.isConnected) { teardown(false); return; }
    if (!inside(e)) return;
    const k = e.key.toLowerCase();
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (k === "f" || k === "h")) {
      // Ctrl+F / Ctrl+H from inside switch tabs rather than open the browser's own find.
      e.stopPropagation(); e.preventDefault();
      fr.tab = k === "h" ? "replace" : "find";
      sync();
      focusFirst();
    } else if (e.key === "F4" && e.shiftKey && !e.altKey) {
      e.stopPropagation(); e.preventDefault();
      findStep(e.ctrlKey || e.metaKey ? -1 : 1);
    } else if (e.key === "Escape") { e.stopPropagation(); e.preventDefault(); close(true); }
    else if (e.key === "Enter") {
      const t = e.composedPath ? e.composedPath()[0] : e.target;
      e.stopPropagation(); e.preventDefault();
      if (t instanceof HTMLButtonElement) t.click();
      else findStep(e.shiftKey ? -1 : 1);
    }
  };
  const onX = () => close(true);

  // Another dialog reusing the frame, or anything hiding it, ends this one.
  const watch = new MutationObserver(() => {
    if (!root.isConnected || modal.hidden) teardown(false);
  });
  watch.observe(body, { childList: true });
  watch.observe(modal, { attributes: true, attributeFilter: ["hidden"] });

  function teardown(hide) {
    if (fr.root !== root) return;
    watch.disconnect();
    document.removeEventListener("keydown", onKey, true);
    x.removeEventListener("click", onX);
    title.removeEventListener("pointerdown", drag);
    title.classList.remove("oc-drag-handle");
    modal.classList.remove("oc-modeless");
    box.removeAttribute("aria-modal");
    box.setAttribute("aria-modal", "true");
    box.style.left = "";
    box.style.top = "";
    fr.root = null;
    fr.sync = null;
    fr.teardown = null;
    findState.open = false;
    findState.matches = [];
    if (hide) {
      modal.hidden = true;
      if (root.isConnected) body.textContent = "";
    }
    draw();
  }
  function close(refocus) {
    teardown(true);
    if (refocus) canvas.focus();
  }
  fr.teardown = close;
  document.addEventListener("keydown", onKey, true);
  x.addEventListener("click", onX);
}

export function colFromLetters(s) {
  let n = 0;
  for (const ch of s.toUpperCase()) {
    if (ch < "A" || ch > "Z") return null;
    n = n * 26 + (ch.charCodeAt(0) - 64);
  }
  return n > 0 ? n - 1 : null;
}

export function parseA1Cell(s) {
  const m = /^([A-Za-z]+)([0-9]+)$/.exec(s.trim());
  if (!m) return null;
  const col = colFromLetters(m[1]);
  const row = parseInt(m[2], 10) - 1;
  return col === null || row < 0 || !Number.isFinite(row) ? null : { row, col };
}

export function parseNameRange(text) {
  const t = (text || "").trim();
  if (!t) return null;
  const b = usedBounds();
  const wholeCols = /^\$?([A-Za-z]{1,3})\s*:\s*\$?([A-Za-z]{1,3})$/.exec(t);
  if (wholeCols) {
    const a = colFromName(wholeCols[1]), z = colFromName(wholeCols[2]);
    if (a === null || z === null) return null;
    return { r0: 0, c0: Math.min(a, z), r1: Math.max(0, b.rows - 1), c1: Math.max(a, z), kind: "cols" };
  }
  const wholeRows = /^\$?(\d+)\s*:\s*\$?(\d+)$/.exec(t);
  if (wholeRows) {
    const a = parseInt(wholeRows[1], 10) - 1, z = parseInt(wholeRows[2], 10) - 1;
    if (a < 0 || z < 0) return null;
    return { r0: Math.min(a, z), c0: 0, r1: Math.max(a, z), c1: Math.max(0, b.cols - 1), kind: "rows" };
  }
  const parts = t.split(":");
  if (parts.length === 2) {
    const p = parseA1Cell(parts[0]), q = parseA1Cell(parts[1]);
    if (!p || !q) return null;
    return {
      r0: Math.min(p.row, q.row), c0: Math.min(p.col, q.col),
      r1: Math.max(p.row, q.row), c1: Math.max(p.col, q.col),
    };
  }
  const c = parseA1Cell(t);
  return c ? { r0: c.row, c0: c.col, r1: c.row, c1: c.col } : null;
}

export function colFromName(letters) {
  let n = 0;
  for (const ch of letters.toUpperCase()) {
    const v = ch.charCodeAt(0) - 64;
    if (v < 1 || v > 26) return null;
    n = n * 26 + v;
  }
  return n - 1;
}

export function gotoName(v) {
  const s = (v || "").trim();
  if (!s) { updateNameBox(); return; }

  // A comma-separated list builds a multi-range selection, as in Excel. The
  // active cell is the top-left of the first range (XL-UX-165): `selectBoxes`
  // makes the *last* box the active one, so the first goes to the end.
  if (s.includes(",")) {
    const parts = s.split(",").map((x) => x.trim()).filter(Boolean);
    const boxes = parts.map(parseNameRange).filter(Boolean);
    if (boxes.length === parts.length && boxes.length > 1) {
      boxes.push(boxes.shift());
      selectBoxes(state.sheet, boxes);
      ensureVisible();
      return;
    }
  }

  // A sheet qualifier moves there first, so `Sheet2!B7` lands on Sheet2.
  let text = s;
  const bang = text.lastIndexOf("!");
  if (bang > 0) {
    const name = text.slice(0, bang).replace(/^'|'$/g, "").replace(/''/g, "'");
    try {
      const idx = JSON.parse(wasm.session_sheet_names())
        .findIndex((n) => n.toLowerCase() === name.toLowerCase());
      if (idx >= 0) { switchSheet(idx); text = text.slice(bang + 1); }
    } catch {}
  }

  // A range — typed, or a defined name's — is selected with its top-left cell
  // active, as Excel and the Go To dialog (`selectBoxes`) do. The active cell
  // used to be the bottom-right, so what was typed next landed there.
  const box = parseNameRange(text);
  if (box) {
    if (box.r0 === box.r1 && box.c0 === box.c1) { select(box.r0, box.c0); return; }
    selectBoxes(state.sheet, [box]);
    ensureVisible();
    return;
  }
  // An existing defined name → jump to its target range.
  try {
    const t = wasm.session_name_target(s);
    if (t !== "null") {
      const r = JSON.parse(t);
      selectBoxes(state.sheet, [{ r0: r.r0, c0: r.c0, r1: r.r1, c1: r.c1 }]);
      ensureVisible();
      return;
    }
  } catch {}
  // A valid new name → define it for the current selection (Excel's name box).
  if (/^[A-Za-z_][A-Za-z0-9_.]*$/.test(s)) {
    const r = effectiveRange();
    const names = JSON.parse(wasm.session_sheet_names());
    const sn = names[state.sheet] || "Sheet1";
    const q = sheetRef(sn);
    const refers = `${q}!${A1(r.r0, r.c0)}:${A1(r.r1, r.c1)}`;
    try { wasm.session_define_name(s, refers); status.textContent = `defined name “${s}”`; }
    catch (e) { statusError(errText(e)); }
    updateNameBox();
    return;
  }
  status.textContent = `Can't go to “${s}” — type a cell (B12), range (A1:C5), or a name`;
  updateNameBox();
}
