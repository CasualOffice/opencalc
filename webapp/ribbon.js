// The Excel shell: title-bar additions, tab strip, ribbon and File page —
// UX-SHELL-06/07/08, built to the owner-approved prototype
// (`docs/prototypes/excel-shell-2026/`, docs/95).
//
// **This chrome owns its controls.** The previous ribbon borrowed the live
// toolbar's DOM nodes and moved them into its own tree, then mirrored their
// state back through a MutationObserver; a control could therefore look only
// one way and live in only one place, and a command that lived only in the
// right-click menu did not exist for it at all (23 of 197). Now every button
// here is drawn from `ribbon.model.js` and runs through the command registry
// (`commands.js`). Nothing in the editor changes parent.
//
// What it still uses from the toolbar is behaviour, never position: a control
// whose popup the toolbar already builds (borders, colours, merge, orientation)
// is clicked, and the popup it opens is re-anchored under the ribbon button.

import { command, run, all as allCommands } from "./commands.js";
import { TABS, FILE_RAIL } from "./ribbon.model.js";

const LAYOUT_KEY = "oc.ribbon.layout";
const LABELS_KEY = "oc.ribbon.labels";

/// One shell (UX-SHELL-08). The toolbar and Sheets chromes are retired; these
/// stay exported because the page bootstrap and the embed API call them.
export function chosenChrome() { return "ribbon"; }
export function setChrome() { /* one shell: nothing to choose */ }
export function ribbonRequested() { return true; }

const store = {
  get(k, d) { try { return localStorage.getItem(k) || d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* private mode */ } },
};

/* ── Icons ──────────────────────────────────────────────────────────────── */

let spritePromise = null;
function ensureSprite() {
  if (document.getElementById("oc-fluent-icons")) return Promise.resolve();
  if (spritePromise) return spritePromise;
  spritePromise = fetch(new URL("./icons.fluent.svg", import.meta.url))
    .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`sprite ${r.status}`))))
    .then((svg) => {
      if (document.getElementById("oc-fluent-icons")) return;
      const host = document.createElement("div");
      // The generated Fluent sprite, fetched from this origin, `<symbol>`s and
      // nothing else; `<use href>` only resolves against parsed symbols.
      // oc-safe-html: static local sprite, no interpolation.
      host.innerHTML = svg;
      const sprite = host.querySelector("svg");
      if (sprite) document.body.insertBefore(sprite, document.body.firstChild);
    })
    .catch((e) => console.warn("[shell] icon sprite failed to load", e));
  return spritePromise;
}

const SVG = "http://www.w3.org/2000/svg";
function icon(name, size) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 20 20");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("class", "rb-ico");
  const use = document.createElementNS(SVG, "use");
  use.setAttribute("href", `#fi-${name}`);
  svg.appendChild(use);
  return svg;
}
const caret = () => { const c = icon("chevron_down", 10); c.classList.add("rb-caret"); return c; };

function h(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

/// "Bold (Ctrl+B)" → "Bold". The model carries Excel's tooltip; the visible
/// label, when labels are on, is the part before the shortcut.
const shortLabel = (s) => s.replace(/\s*\(.*\)\s*$/, "");

/* ── Popovers ───────────────────────────────────────────────────────────── */

let openPop = null;
function closePop() {
  if (!openPop) return;
  const { el, owner, onClose } = openPop;
  openPop = null;
  el.remove();
  owner?.setAttribute("aria-expanded", "false");
  onClose?.();
}

/// Below `anchor` if it fits, above if not, clamped to the window — the same
/// rule the editor's own `anchorMenu` follows.
function place(el, anchor) {
  const r = anchor.getBoundingClientRect();
  el.style.left = "0px";
  el.style.top = "0px";
  const w = el.offsetWidth, ht = el.offsetHeight;
  let left = r.left;
  if (left + w > window.innerWidth - 4) left = Math.max(4, window.innerWidth - 4 - w);
  let top = r.bottom + 2;
  if (top + ht > window.innerHeight - 4) top = r.top - 2 - ht >= 4 ? r.top - 2 - ht : Math.max(4, window.innerHeight - 4 - ht);
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
}

function popover(owner, build, onClose) {
  closePop();
  const el = h("div", "popmenu rb-pop");
  el.setAttribute("role", "menu");
  build(el);
  document.body.appendChild(el);
  place(el, owner);
  owner.setAttribute("aria-expanded", "true");
  openPop = { el, owner, onClose };
  el.querySelector("button, input")?.focus({ preventScroll: true });
  return el;
}

/// A dropdown listing commands by id, each run through the registry. Absent
/// ids are skipped; a disabled one is shown disabled in place.
function commandMenu(owner, ids) {
  popover(owner, (el) => {
    for (const id of ids) {
      const c = command(id);
      if (!c || (c.hidden && c.hidden())) continue;   // a rule took it away
      const b = h("button", "rb-pop-item", c.label || id);
      b.type = "button";
      b.dataset.cmd = id;
      b.setAttribute("role", "menuitem");
      if (c.shortcut) b.appendChild(h("span", "rb-pop-key", c.shortcut));
      if (c.enabled && !c.enabled()) b.disabled = true;
      if (c.checked && c.checked()) b.setAttribute("aria-checked", "true");
      b.addEventListener("click", (e) => { e.stopPropagation(); closePop(); run(id); refocusGrid(); });
      el.appendChild(b);
    }
  });
}

/// A toolbar control that owns a popup: click it, then move the popup it
/// opened under the ribbon button. The popup is `position: fixed`, so where it
/// is drawn is independent of where its owner sits.
function toolbarPopup(owner, id) {
  const c = command(id);
  if (!c?.node) return;
  closePop();
  c.node.click();
  const menu = c.node.closest(".menu-wrap")?.querySelector(".popmenu:not([hidden])");
  if (menu) place(menu, owner);
}

function refocusGrid() {
  document.getElementById("grid")?.focus({ preventScroll: true });
}

/* ── Controls ───────────────────────────────────────────────────────────── */

const sync = new Set();   // functions that refresh a control's state

function button(spec, size) {
  const b = h("button", `rb-btn rb-${size}`);
  b.type = "button";
  b.dataset.cmd = spec.id;
  b.dataset.ocProxy = spec.id;   // forwards to this command; never owns it
  b.title = spec.label;
  b.setAttribute("aria-label", shortLabel(spec.label));
  b.appendChild(icon(spec.icon, size === "l" ? 24 : 16));
  if (size === "l") b.appendChild(h("span", "rb-label", shortLabel(spec.label)));
  const drops = spec.menu || spec.popup;
  if (drops) { b.appendChild(caret()); b.setAttribute("aria-haspopup", "true"); b.setAttribute("aria-expanded", "false"); }
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    if (spec.popup) toolbarPopup(b, spec.id);
    else if (spec.menu) commandMenu(b, spec.menu);
    else { closePop(); run(spec.id); refocusGrid(); queueSync(); }
  });
  const c = command(spec.id);
  if (!c && !spec.menu) b.hidden = true;
  // A colour control shows its current colour as a bar under the icon, as in
  // Excel, read from the editor's own control so there is one source of it.
  if (spec.popup && /color|border/.test(spec.id)) {
    b.classList.add("rb-swatch");
    sync.add(() => {
      const src = command(spec.id)?.node;
      if (!src) return;
      for (const v of ["--oc-x-border-swatch", "--oc-x-font-swatch", "--oc-x-fill-swatch"]) {
        const val = src.style.getPropertyValue(v);
        if (val) b.style.setProperty("--rb-swatch", val);
      }
    });
  }
  sync.add(() => {
    // A dropdown is only as present as what it drops: when every item is
    // absent or hidden by the rules, the button goes too (an empty menu is
    // worse than none, as `applyCommandRules()` says of the menu bar).
    if (spec.menu) {
      b.hidden = !spec.menu.some((id) => { const m = command(id); return m && !(m.hidden && m.hidden()); });
    }
    const cc = command(spec.id);
    if (!cc) return;
    // The editor's rules (read-only, a host's hidden list, a mode's
    // capabilities) decide; a hidden command leaves the ribbon entirely.
    if (!spec.menu) b.hidden = cc.hidden ? cc.hidden() : false;
    b.disabled = cc.enabled ? !cc.enabled() : false;
    if (cc.checked) b.setAttribute("aria-pressed", cc.checked() ? "true" : "false");
  });
  return b;
}

function combo(spec) {
  const c = command(spec.id);
  const wrap = h("div", "rb-combo");
  wrap.style.width = `${spec.width}px`;
  const input = h("input", "rb-combo-input");
  input.type = "text";
  input.spellcheck = false;
  input.setAttribute("aria-label", spec.label);
  input.title = spec.label;
  input.dataset.cmd = spec.id;
  input.dataset.ocProxy = spec.id;
  if (c?.readOnly) input.readOnly = true;
  const btn = h("button", "rb-combo-caret");
  btn.type = "button";
  btn.tabIndex = -1;
  btn.setAttribute("aria-label", `Show ${spec.label.toLowerCase()} list`);
  btn.appendChild(caret());
  wrap.append(input, btn);
  if (!c) { wrap.hidden = true; return wrap; }

  const commit = (v) => { closePop(); c.run(v); queueSync(); refocusGrid(); };
  const list = (filter) => popover(wrap, (el) => {
    el.classList.add("rb-combo-list");
    el.style.minWidth = `${Math.max(spec.width, 140)}px`;
    const f = (filter || "").toLowerCase();
    const cur = c.current ? c.current() : "";
    for (const o of c.values()) {
      if (f && !o.label.toLowerCase().includes(f)) continue;
      const b = h("button", "rb-pop-item", o.label);
      b.type = "button";
      b.setAttribute("role", "option");
      if (o.font) b.style.fontFamily = o.font;
      if (o.label === cur || o.v === cur) b.setAttribute("aria-selected", "true");
      b.addEventListener("mousedown", (e) => e.preventDefault());
      b.addEventListener("click", (e) => { e.stopPropagation(); input.value = o.label; commit(o.v); });
      el.appendChild(b);
    }
  });
  btn.addEventListener("mousedown", (e) => e.preventDefault());
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (openPop?.owner === wrap) { closePop(); return; }
    list("");
    input.focus({ preventScroll: true });
  });
  input.addEventListener("click", (e) => { e.stopPropagation(); if (input.readOnly) list(""); });
  input.addEventListener("focus", () => input.select());
  input.addEventListener("input", () => { if (!input.readOnly) list(input.value); input.focus(); });
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();   // the grid's global handler must not see these
    if (e.key === "Enter") { e.preventDefault(); if (!input.readOnly) commit(input.value.trim()); }
    else if (e.key === "Escape") { e.preventDefault(); closePop(); queueSync(); refocusGrid(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); list(""); }
  });
  sync.add(() => {
    if (document.activeElement === input) return;
    input.value = c.current ? c.current() : "";
  });
  return wrap;
}

function rows(spec) {
  const col = h("div", "rb-rows");
  col.dataset.rows = String(spec.rows.length);
  for (const r of spec.rows) {
    const row = h("div", "rb-row");
    for (const it of r) row.appendChild(it.kind === "combo" ? combo(it) : button(it, "s"));
    col.appendChild(row);
  }
  return col;
}

function group(spec) {
  const g = h("div", "rb-group");
  g.setAttribute("role", "group");
  g.setAttribute("aria-label", spec.label);
  g.dataset.fold = String(spec.fold ?? 99);
  const items = h("div", "rb-items");
  for (const it of spec.items) items.appendChild(it.kind === "large" ? button(it, "l") : rows(it));
  const cap = h("div", "rb-cap");
  cap.appendChild(h("span", "rb-cap-text", spec.label));
  g.append(items, cap);
  if (spec.launcher && command(spec.launcher)) {
    const l = h("button", "rb-launch");
    l.type = "button";
    l.title = `${spec.label} settings`;
    l.setAttribute("aria-label", `${spec.label} settings`);
    l.appendChild(icon("arrow_down", 10));
    l.addEventListener("click", (e) => { e.stopPropagation(); closePop(); run(spec.launcher); });
    g.appendChild(l);
  }
  // The folded form: one large button that drops the group's own contents.
  const folded = h("button", "rb-btn rb-l rb-folded");
  folded.type = "button";
  folded.title = spec.label;
  folded.setAttribute("aria-label", spec.label);
  folded.setAttribute("aria-haspopup", "true");
  folded.setAttribute("aria-expanded", "false");
  folded.append(icon(spec.icon || "more_horizontal", 24), h("span", "rb-label", spec.label), caret());
  folded.addEventListener("click", (e) => {
    e.stopPropagation();
    if (openPop?.owner === folded) { closePop(); return; }
    // The live items move into the popover and back on close, so they keep
    // their listeners and state; they are this chrome's own nodes.
    popover(folded, (el) => { el.classList.add("rb-fold-pop"); el.appendChild(items); }, () => g.insertBefore(items, cap));
  });
  g.insertBefore(folded, items);
  return g;
}

/* ── Build ──────────────────────────────────────────────────────────────── */

let el = null;          // the ribbon
let backstage = null;
let titleBits = [];     // nodes added to the header, removed by stop()
let cleanups = [];
let syncQueued = false;

function queueSync() {
  if (syncQueued) return;
  syncQueued = true;
  requestAnimationFrame(() => { syncQueued = false; for (const f of sync) f(); });
}

/// Fold groups, lowest `fold` first, until the active panel fits.
function fit(panel) {
  if (!panel) return;
  for (const g of panel.querySelectorAll(".rb-group.is-folded")) g.classList.remove("is-folded");
  const groups = [...panel.querySelectorAll(".rb-group")].sort((a, b) => a.dataset.fold - b.dataset.fold);
  for (const g of groups) {
    if (panel.scrollWidth <= panel.clientWidth + 1) break;
    g.classList.add("is-folded");
  }
}

function buildTitleBar() {
  const header = document.querySelector(".app-header");
  if (!header) return;
  const logo = header.querySelector(".brand-logo");
  // Quick Access Toolbar: Save, Undo, Redo — Excel's default three.
  const qat = h("div", "rb-qat");
  qat.setAttribute("role", "toolbar");
  qat.setAttribute("aria-label", "Quick Access Toolbar");
  // Save is Excel's first QAT button, and what it does depends on where the
  // document lives: write the file when there is one, otherwise download it in
  // the format it was opened in, otherwise as .xlsx. Hidden only when the mode
  // allows none of them.
  const SAVE_ROUTES = ["file.save", "file.download.same-format-as-opened", "file.download.excel-xlsx"];
  const saveRoute = () => SAVE_ROUTES.find((id) => { const c = command(id); return c && !(c.hidden && c.hidden()); });
  const save = h("button", "rb-btn rb-s");
  save.type = "button";
  save.title = "Save (Ctrl+S)";
  save.setAttribute("aria-label", "Save");
  save.dataset.ocProxy = "file.save";
  save.appendChild(icon("save", 16));
  save.addEventListener("click", (e) => { e.stopPropagation(); const id = saveRoute(); if (id) run(id); refocusGrid(); });
  sync.add(() => { save.hidden = !saveRoute(); });
  qat.appendChild(save);
  for (const [id, ic, label] of [["edit.undo", "arrow_undo", "Undo (Ctrl+Z)"], ["edit.redo", "arrow_redo", "Redo (Ctrl+Y)"]]) {
    qat.appendChild(button({ id, icon: ic, label }, "s"));
  }
  (logo ? logo.after(qat) : header.prepend(qat));
  titleBits.push(qat);

  // Search (Alt+Q): every command the registry can run, by name.
  const search = h("div", "rb-search");
  const input = h("input", "rb-search-input");
  input.type = "search";
  input.placeholder = "Search (Alt+Q)";
  input.setAttribute("aria-label", "Search commands");
  search.append(icon("search", 16), input);
  const results = (q) => {
    const f = q.trim().toLowerCase();
    if (!f) { closePop(); return; }
    const words = f.split(/\s+/);
    const hits = allCommands()
      .filter((c) => c.label && words.every((w) => c.label.toLowerCase().includes(w) || c.id.includes(w)))
      .slice(0, 8);
    popover(search, (el2) => {
      el2.classList.add("rb-search-results");
      if (!hits.length) el2.appendChild(h("div", "rb-pop-empty", "No matching commands"));
      for (const c of hits) {
        const b = h("button", "rb-pop-item", c.label);
        b.type = "button";
        b.setAttribute("role", "menuitem");
        if (c.shortcut) b.appendChild(h("span", "rb-pop-key", c.shortcut));
        b.addEventListener("mousedown", (e) => e.preventDefault());
        b.addEventListener("click", (e) => { e.stopPropagation(); closePop(); input.value = ""; run(c.id); refocusGrid(); });
        el2.appendChild(b);
      }
    });
    input.focus();
  };
  input.addEventListener("input", () => results(input.value));
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") { e.preventDefault(); openPop?.el.querySelector(".rb-pop-item")?.click(); }
    else if (e.key === "Escape") { e.preventDefault(); closePop(); input.value = ""; refocusGrid(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); openPop?.el.querySelector(".rb-pop-item")?.focus(); }
  });
  const right = header.querySelector(".header-right");
  header.insertBefore(search, right);
  titleBits.push(search);
  const altQ = (e) => {
    // Stopped here: on a Mac Alt+Q is "œ", which the grid would otherwise
    // take as the first character of a cell edit and steal focus back.
    if (e.altKey && (e.key === "q" || e.key === "Q" || e.code === "KeyQ")) { e.preventDefault(); e.stopPropagation(); input.focus(); }
  };
  document.addEventListener("keydown", altQ, true);
  cleanups.push(() => document.removeEventListener("keydown", altQ, true));

  if (right) {
    // Present only while the mode allows sharing: a host that turns
    // `canShare` off (embedded, WOPI, or later at runtime) takes the button
    // away rather than leaving one that can only refuse.
    const share = h("button", "rb-share");
    share.type = "button";
    share.dataset.ocProxy = "file.share";
    share.append(icon("share", 16), h("span", null, "Share"));
    share.addEventListener("click", () => run("file.share"));
    right.prepend(share);
    titleBits.push(share);
    const paint = () => {
      const c = command("file.share");
      share.hidden = !c || (c.hidden && c.hidden());
    };
    paint();
    sync.add(paint);
  }
}

/// Who else is here. The roster's markup lives in the menu bar, which this
/// shell parks off-screen — left there, nobody could see a co-editor. It sits
/// beside Share in the title bar, as in Excel; a host that hides the title bar
/// (`hide: header`, embedded mode) still gets it, at the end of the tab row,
/// because who you are working with is not chrome (`UX-CHROME-01`).
/// Moved, not copied: `renderPresence` keeps its node, and `stop()` puts it
/// back where it came from.
function seatPresence(strip) {
  const presence = document.getElementById("presence");
  if (!presence) return;
  const home = presence.parentNode, next = presence.nextSibling;
  cleanups.push(() => home.insertBefore(presence, next));
  const seat = () => {
    const header = document.querySelector(".app-header");
    const right = header?.querySelector(".header-right");
    const shown = header && right && getComputedStyle(header).display !== "none";
    const target = shown ? right : strip;
    if (presence.parentNode !== target) target[shown ? "prepend" : "append"](presence);
  };
  seat();
  sync.add(seat);
}

function buildBackstage() {
  const bs = h("div", "rb-backstage");
  bs.hidden = true;
  bs.setAttribute("role", "dialog");
  bs.setAttribute("aria-label", "File");
  const rail = h("nav", "rb-rail");
  const back = h("button", "rb-back");
  back.type = "button";
  back.title = "Back (Esc)";
  back.setAttribute("aria-label", "Back to the workbook");
  back.appendChild(icon("chevron_left", 18));
  back.addEventListener("click", () => closeBackstage());
  rail.appendChild(back);
  const pane = h("div", "rb-pane");
  bs.append(rail, pane);

  const pages = [];
  const show = (id) => {
    for (const [pid, item, page] of pages) {
      const on = pid === id;
      item.classList.toggle("is-on", on);
      item.setAttribute("aria-current", on ? "page" : "false");
      page.hidden = !on;
    }
  };
  for (const entry of FILE_RAIL) {
    const page = h("section", "rb-page");
    page.hidden = true;
    page.appendChild(h("h1", "rb-page-title", entry.label));
    if (entry.id === "new") {
      const tiles = h("div", "rb-tiles");
      const tile = (ic, label, fn) => {
        const t = h("button", "rb-tile");
        t.type = "button";
        const art = h("span", "rb-tile-art");
        art.appendChild(icon(ic, 40));
        t.append(art, h("span", "rb-tile-label", label));
        t.addEventListener("click", () => { closeBackstage(); fn(); });
        tiles.appendChild(t);
      };
      if (command("file.new")) tile("grid", "Blank workbook", () => run("file.new"));
      if (command("file.open")) tile("folder_open", "Open a file", () => run("file.open"));
      page.appendChild(tiles);
    } else {
      const list = h("div", "rb-page-list");
      for (const id of entry.cmds || []) {
        const c = command(id);
        if (!c) continue;
        const b = h("button", "rb-page-item", c.label || id);
        b.type = "button";
        b.dataset.ocProxy = id;
        if (c.checked && c.checked()) b.setAttribute("aria-checked", "true");
        b.addEventListener("click", () => { closeBackstage(); run(id); });
        list.appendChild(b);
      }
      if (entry.id === "options") list.appendChild(labelsChoice());
      if (!list.children.length) continue;
      page.appendChild(list);
    }
    const item = h("button", "rb-rail-item");
    item.type = "button";
    item.append(icon(entry.icon, 18), h("span", null, entry.label));
    item.addEventListener("click", () => show(entry.id));
    rail.appendChild(item);
    pane.appendChild(page);
    pages.push([entry.id, item, page]);
  }
  bs.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeBackstage(); } });
  if (pages.length) show(pages[0][0]);
  return bs;
}

/// File > Options > Ribbon labels. Excel draws group names and large-button
/// labels; the owner's default is icons only (docs/95 rule 1).
function labelsChoice() {
  const box = h("div", "rb-choice");
  box.setAttribute("role", "radiogroup");
  box.setAttribute("aria-label", "Ribbon labels");
  box.appendChild(h("span", "rb-choice-title", "Ribbon labels"));
  for (const [v, label] of [["none", "Icons only"], ["captions", "Group names"], ["full", "Group names and button labels"]]) {
    const b = h("button", "rb-page-item", label);
    b.type = "button";
    b.setAttribute("role", "radio");
    const paint = () => b.setAttribute("aria-checked", store.get(LABELS_KEY, "none") === v ? "true" : "false");
    paint();
    b.addEventListener("click", () => { applyLabels(v); for (const x of box.querySelectorAll("[role=radio]")) x.dispatchEvent(new Event("oc-paint")); });
    b.addEventListener("oc-paint", paint);
    box.appendChild(b);
  }
  return box;
}

function applyLabels(v) {
  store.set(LABELS_KEY, v);
  if (!el) return;
  el.classList.toggle("rb-labels-captions", v === "captions" || v === "full");
  el.classList.toggle("rb-labels-full", v === "full");
  fit(el.querySelector(".rb-panel:not([hidden])"));
}

function openBackstage() {
  if (!backstage) return;
  closePop();
  backstage.hidden = false;
  document.documentElement.classList.add("oc-backstage-open");
  backstage.querySelector(".rb-rail-item.is-on, .rb-rail-item")?.focus();
}

function closeBackstage() {
  // Closing a page that is not open must not move focus: this runs on every
  // Escape, including the one that cancels a cell edit (UX-SHELL-01).
  if (!backstage || backstage.hidden) return;
  backstage.hidden = true;
  document.documentElement.classList.remove("oc-backstage-open");
  el?.querySelector(".rb-file")?.focus();
}

/* ── Phones: Excel mobile's bottom command bar (docs/95, prototype Phone) ── */

/// Every runnable control on a tab, flattened in ribbon order: what the phone
/// bar shows as icons and the expanded sheet lists with labels.
function flatten(tab) {
  const out = [];
  const walk = (it) => {
    if (it.kind === "rows") { it.rows.forEach((r) => r.forEach(walk)); return; }
    if (it.kind === "combo") return;
    out.push(it);
  };
  for (const g of tab.groups) g.items.forEach(walk);
  return out;
}

let mobile = null;
function buildMobile(select, current) {
  const bar = h("div", "rb-mobile");
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", "Commands");
  const sheet = h("div", "rb-msheet");
  sheet.hidden = true;
  const chips = h("div", "rb-mchips");
  const list = h("div", "rb-mlist");
  sheet.append(chips, list);
  const row = h("div", "rb-mrow");
  const pick = h("button", "rb-mpick");
  pick.type = "button";
  pick.setAttribute("aria-haspopup", "true");
  pick.setAttribute("aria-expanded", "false");
  const pickLabel = h("span");
  pick.append(pickLabel, caret());
  const icons = h("div", "rb-micons");
  row.append(pick, icons);
  bar.append(sheet, row);

  const runItem = (it, anchor) => {
    if (it.popup) toolbarPopup(anchor, it.id);
    else if (it.menu) commandMenu(anchor, it.menu);
    else { run(it.id); refocusGrid(); queueSync(); }
  };
  const paint = () => {
    const tab = TABS.find((t) => t.id === current()) || TABS[0];
    pickLabel.textContent = tab.label;
    icons.textContent = "";
    list.textContent = "";
    for (const it of flatten(tab)) {
      const c = command(it.id);
      const live = it.menu ? it.menu.some((id) => { const m = command(id); return m && !(m.hidden && m.hidden()); })
        : c && !(c.hidden && c.hidden());
      if (!live) continue;
      const b = h("button", "rb-mbtn");
      b.type = "button";
      b.title = it.label;
      b.setAttribute("aria-label", shortLabel(it.label));
      b.dataset.ocProxy = it.id;
      b.appendChild(icon(it.icon, 22));
      if (c?.checked) b.setAttribute("aria-pressed", c.checked() ? "true" : "false");
      b.addEventListener("click", (e) => { e.stopPropagation(); runItem(it, b); });
      icons.appendChild(b);
      const li = h("button", "rb-mitem");
      li.type = "button";
      li.dataset.cmd = it.id;
      li.append(icon(it.icon, 20), h("span", null, shortLabel(it.label)));
      if (it.menu || it.popup) { li.setAttribute("aria-haspopup", "true"); li.appendChild(icon("chevron_right", 16)); }
      li.addEventListener("click", (e) => { e.stopPropagation(); runItem(it, li); });
      list.appendChild(li);
    }
    // Built once: every sync repaints, and rebuilding the chips each time took
    // keyboard focus out of an open sheet and detached the chip under a finger.
    if (!chips.firstChild) {
      const file = h("button", "rb-mchip", "File");
      file.type = "button";
      file.addEventListener("click", () => { toggle(false); openBackstage(); });
      chips.appendChild(file);
      for (const t of TABS) {
        const chip = h("button", "rb-mchip", t.label);
        chip.type = "button";
        chip.dataset.tab = t.id;
        chip.addEventListener("click", (e) => { e.stopPropagation(); select(t.id); paint(); });
        chips.appendChild(chip);
      }
    }
    for (const chip of chips.querySelectorAll("[data-tab]")) {
      chip.setAttribute("aria-pressed", chip.dataset.tab === tab.id ? "true" : "false");
    }
  };
  const toggle = (open) => {
    sheet.hidden = !open;
    pick.setAttribute("aria-expanded", open ? "true" : "false");
    bar.classList.toggle("is-open", open);
  };
  pick.addEventListener("click", (e) => { e.stopPropagation(); toggle(sheet.hidden); });
  bar._paint = paint;
  bar._toggle = toggle;
  paint();
  return bar;
}

export async function start() {
  if (el) return el;
  await ensureSprite();
  const anchor = document.querySelector(".formula-bar");
  if (!anchor) return null;
  sync.clear();

  el = h("div", "oc-ribbon");
  const strip = h("div", "rb-tabs");
  const file = h("button", "rb-file", "File");
  file.type = "button";
  file.setAttribute("aria-haspopup", "dialog");
  file.addEventListener("click", () => openBackstage());
  const tablist = h("div", "rb-tablist");
  tablist.setAttribute("role", "tablist");
  tablist.setAttribute("aria-label", "Ribbon");
  strip.append(file, tablist);

  const band = h("div", "rb-band");
  const panels = new Map();
  for (const tab of TABS) {
    const t = h("button", "rb-tab", tab.label);
    t.type = "button";
    t.id = `rb-tab-${tab.id}`;
    t.setAttribute("role", "tab");
    t.setAttribute("aria-controls", `rb-panel-${tab.id}`);
    t.dataset.key = tab.key;
    t.addEventListener("click", () => select(tab.id));
    tablist.appendChild(t);
    const panel = h("div", "rb-panel");
    panel.id = `rb-panel-${tab.id}`;
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", t.id);
    panel.hidden = true;
    for (const g of tab.groups) panel.appendChild(group(g));
    band.appendChild(panel);
    panels.set(tab.id, panel);
  }
  const layout = h("button", "rb-layout");
  layout.type = "button";
  layout.appendChild(icon("chevron_down", 14));
  band.appendChild(layout);
  el.append(strip, band);
  anchor.parentElement.insertBefore(el, anchor);

  buildTitleBar();
  backstage = buildBackstage();
  document.body.appendChild(backstage);
  document.documentElement.classList.add("oc-chrome-ribbon");

  function select(id) {
    closePop();
    for (const [k, p] of panels) p.hidden = k !== id;
    for (const b of tablist.querySelectorAll(".rb-tab")) {
      const on = b.id === `rb-tab-${id}`;
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-selected", on ? "true" : "false");
      b.tabIndex = on ? 0 : -1;
    }
    fit(panels.get(id));
    queueSync();
  }
  el._select = select;

  const applyLayout = (v) => {
    el.classList.toggle("rb-simplified", v === "simplified");
    layout.title = v === "simplified" ? "Show the full ribbon" : "Show a simplified ribbon";
    layout.setAttribute("aria-label", layout.title);
    store.set(LAYOUT_KEY, v);
  };
  layout.addEventListener("click", () => {
    applyLayout(el.classList.contains("rb-simplified") ? "classic" : "simplified");
    fit(el.querySelector(".rb-panel:not([hidden])"));
  });
  applyLayout(store.get(LAYOUT_KEY, "classic"));
  applyLabels(store.get(LABELS_KEY, "none"));

  // Arrow keys move between tabs; the band is reached with Tab.
  tablist.addEventListener("keydown", (e) => {
    const tabs = [...tablist.querySelectorAll(".rb-tab")];
    const i = tabs.indexOf(document.activeElement);
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (i < 0 || !step) return;
    e.preventDefault();
    const next = tabs[(i + step + tabs.length) % tabs.length];
    next.focus();
    next.click();
  });

  // KeyTips: Alt shows a letter on File and every tab; the letter opens it.
  let tips = false;
  const showTips = (on) => {
    tips = on;
    el.classList.toggle("rb-keytips", on);
    for (const t of [file, ...tablist.querySelectorAll(".rb-tab")]) {
      let tip = t.querySelector(".rb-tip");
      if (on && !tip) { tip = h("span", "rb-tip", t === file ? "F" : t.dataset.key); t.appendChild(tip); }
      else if (!on && tip) tip.remove();
    }
  };
  // Shown on the *release* of a lone Alt, as Excel does, so Alt as a modifier
  // (Alt+=, Alt+Enter, Alt+Q) never raises them or loses its keystroke.
  let altAlone = false;
  const onKeyUp = (e) => { if (e.key === "Alt" && altAlone) { altAlone = false; showTips(!tips); } };
  document.addEventListener("keyup", onKeyUp);
  cleanups.push(() => document.removeEventListener("keyup", onKeyUp));
  const onKey = (e) => {
    if (e.key === "Alt") { if (!e.repeat) altAlone = true; return; }
    altAlone = false;
    if (e.key === "Escape") {
      if (tips) { showTips(false); return; }
      if (openPop) { e.preventDefault(); closePop(); refocusGrid(); return; }
      // The editor's own context, sheet-tab and Name Manager menus open as
      // `#sheet-ctx` and only an outside click closed them (UX-SHELL-09).
      const ctx = document.getElementById("sheet-ctx");
      if (ctx) { e.preventDefault(); ctx.remove(); refocusGrid(); return; }
      // A toolbar popup the ribbon opened and re-anchored (colours, borders,
      // orientation, merge) closes the same way (fit-gap XL-UX-004).
      const tbPop = [...document.querySelectorAll(".toolbar .popmenu")].filter((m) => !m.hidden);
      if (tbPop.length) { e.preventDefault(); for (const m of tbPop) m.hidden = true; refocusGrid(); return; }
      closeBackstage();
      return;
    }
    if (!tips) return;
    if (e.ctrlKey || e.metaKey || e.altKey) { showTips(false); return; }
    const k = e.key.toUpperCase();
    e.preventDefault();
    showTips(false);
    if (k === "F") { openBackstage(); return; }
    const tab = TABS.find((t) => t.key === k);
    if (tab) { select(tab.id); document.getElementById(`rb-tab-${tab.id}`)?.focus(); }
  };
  document.addEventListener("keydown", onKey);
  cleanups.push(() => document.removeEventListener("keydown", onKey));

  const onDocClick = (e) => { if (openPop && !openPop.el.contains(e.target)) closePop(); };
  document.addEventListener("click", onDocClick);
  cleanups.push(() => document.removeEventListener("click", onDocClick));

  let fitQueued = false;
  const onResize = () => {
    if (fitQueued) return;
    fitQueued = true;
    requestAnimationFrame(() => { fitQueued = false; closePop(); fit(el?.querySelector(".rb-panel:not([hidden])")); });
  };
  window.addEventListener("resize", onResize);
  cleanups.push(() => window.removeEventListener("resize", onResize));

  // State follows the editor: selection moves, edits, undo/redo, and any
  // toolbar toggle changing its pressed/disabled state.
  const ed = window.opencalcEditor;
  for (const name of ["selectionChanged", "cellsChanged", "undoStateChanged"]) {
    const off = ed?.on?.(name, queueSync);
    if (typeof off === "function") cleanups.push(off);
  }
  document.addEventListener("oc:command-rules", queueSync);
  cleanups.push(() => document.removeEventListener("oc:command-rules", queueSync));
  const obs = new MutationObserver(queueSync);
  for (const n of document.querySelectorAll(".toolbar [data-oc-command], #menubar [data-oc-command]")) {
    obs.observe(n, { attributes: true, attributeFilter: ["disabled", "aria-pressed", "aria-checked", "class", "style"] });
  }
  cleanups.push(() => obs.disconnect());

  // Phones get Excel mobile's bottom bar instead of the ribbon band.
  let currentTab = TABS[0].id;
  const realSelect = select;
  el._select = (id) => { currentTab = id; realSelect(id); mobile?._paint(); };
  mobile = buildMobile((id) => el._select(id), () => currentTab);
  (document.querySelector(".bottom-bar") || document.body.lastElementChild).after(mobile);
  const phone = window.matchMedia("(max-width: 600px)");
  const applyPhone = () => document.documentElement.classList.toggle("oc-phone", phone.matches);
  phone.addEventListener("change", applyPhone);
  cleanups.push(() => phone.removeEventListener("change", applyPhone));
  applyPhone();
  for (const t of tablist.querySelectorAll(".rb-tab")) {
    t.addEventListener("click", () => { currentTab = t.id.replace("rb-tab-", ""); mobile?._paint(); });
  }
  sync.add(() => mobile?._paint());
  seatPresence(strip);

  select(TABS[0].id);
  return el;
}

export function stop() {
  closePop();
  for (const f of cleanups.splice(0)) { try { f(); } catch { /* best effort */ } }
  for (const n of titleBits.splice(0)) n.remove();
  mobile?.remove();
  mobile = null;
  document.documentElement.classList.remove("oc-phone");
  backstage?.remove();
  backstage = null;
  el?.remove();
  el = null;
  sync.clear();
  document.documentElement.classList.remove("oc-chrome-ribbon", "oc-backstage-open");
}

export function toggle(on) { on ? start() : stop(); }

/// For gates: what the ribbon names and the registry cannot run.
export function missingCommandsForTest() {
  const out = [];
  const walk = (it) => {
    if (it.kind === "rows") { it.rows.forEach((r) => r.forEach(walk)); return; }
    if (!it.menu && !command(it.id)) out.push(it.id);
    for (const m of it.menu || []) if (!command(m)) out.push(m);
  };
  for (const t of TABS) for (const g of t.groups) { g.items.forEach(walk); if (g.launcher && !command(g.launcher)) out.push(g.launcher); }
  for (const r of FILE_RAIL) for (const c of r.cmds || []) if (!command(c)) out.push(c);
  return [...new Set(out)];
}
