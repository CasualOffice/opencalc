/// The ribbon chrome (`UX-RIB-04`, [docs/91](../docs/91-EXCEL-RIBBON-CHROME-DESIGN.md)).
///
/// **This module draws a ribbon. It does not implement a single command.**
///
/// That is the whole design, and it is why this file is short. Every verb the
/// editor has already exists as a node carrying `data-oc-command`, already
/// wired, already gated by `applyCommandRules()`, already counted by
/// `listCommands()`. A ribbon that re-implemented Bold would be a second Bold
/// that drifts from the first; a ribbon that re-implemented Freeze would be a
/// worse Freeze. So this does neither:
///
/// - **A live toolbar control is *moved* into the ribbon**, and moved back when
///   the ribbon goes away. It keeps its listeners, its state mirroring and its
///   identity, because it is the same node — the pattern `applyModeChrome()`
///   already uses for desktop chrome, and for the same reason.
/// - **A menu-only command gets a proxy button** that clicks the real menu item.
///   The proxy carries no `data-oc-command` of its own: ADR-026 fixes ids as
///   slugs of the English menu path, and both the capability table and the
///   read-only whitelist match them **by regex**, so minting `ribbon.bold`
///   beside `toolbar.bold` would leave a viewer-mode editor able to save.
///
/// The consequence worth stating plainly: nothing below the chrome changes.
/// Freeze still drags, the header context menu still re-targets, undo still
/// goes through the transaction layer — because none of it was touched.
///
/// Additive and reversible. Absent `?chrome=ribbon` this module does nothing at
/// all, and `stop()` puts every borrowed node back where its markup had it.

const PARAMS = new URL(location.href).searchParams;

/// Where each borrowed node lived before the ribbon took it.
///
/// Keyed by the node and holding the *next sibling* rather than an index, for
/// the reason `editor.core.js` gives about its own map: an index is wrong the
/// moment anything else in that parent moves.
const home = new WeakMap();

function borrow(node, into) {
  if (!home.has(node)) {
    home.set(node, { parent: node.parentElement, next: node.nextElementSibling });
  }
  into.appendChild(node);
}

function giveBack(node) {
  const h = home.get(node);
  if (!h || !h.parent) return;
  // `insertBefore(node, null)` appends, which is right for a node that was the
  // last child — and `next` is `null` in exactly that case.
  h.parent.insertBefore(node, h.next);
}

const byCommand = (id) => document.querySelector(`[data-oc-command="${CSS.escape(id)}"]`);

/// The Home tab, authored by hand against Excel's own group names and order.
///
/// Excel's grouping is the point of the exercise: somebody arriving from Excel
/// must find Merge & Center under Alignment and the number-format box under
/// Number, because that is where twenty years of muscle memory puts them. The
/// ids are this editor's real ones, verified against a running editor rather
/// than guessed — a control named here that does not exist is skipped and
/// counted, never drawn as a dead button.
const HOME = [
  { label: "Clipboard", items: [
    { cmd: "edit.paste", size: "large" },
    { cmd: "edit.cut" }, { cmd: "edit.copy" }, { cmd: "toolbar.painter" },
  ] },
  { label: "Font", items: [
    { cmd: "toolbar.font" }, { cmd: "toolbar.size" },
    { cmd: "toolbar.size-up" }, { cmd: "toolbar.size-down" },
    { cmd: "toolbar.bold" }, { cmd: "toolbar.italic" },
    { cmd: "toolbar.underline" }, { cmd: "toolbar.strike" },
    { cmd: "toolbar.border" }, { cmd: "toolbar.fillcolor" }, { cmd: "toolbar.fontcolor" },
  ] },
  { label: "Alignment", items: [
    { cmd: "toolbar.valign" }, { cmd: "toolbar.rotate" },
    { cmd: "toolbar.indent-less" }, { cmd: "toolbar.indent-more" },
    { cmd: "toolbar.wrap" }, { cmd: "toolbar.merge" },
  ] },
  { label: "Number", items: [
    { cmd: "toolbar.numfmt" }, { cmd: "toolbar.currency" }, { cmd: "toolbar.percent" },
    { cmd: "toolbar.comma" }, { cmd: "toolbar.inc-dec" }, { cmd: "toolbar.dec-dec" },
  ] },
  { label: "Styles", items: [
    { cmd: "format.conditional-formatting", size: "large" },
    { cmd: "format.cell-styles" },
  ] },
  { label: "Editing", items: [
    { cmd: "toolbar.sort" }, { cmd: "toolbar.filter" },
    { cmd: "edit.find-replace" }, { cmd: "edit.clear" }, { cmd: "edit.select-all" },
  ] },
];

/// The tabs after Home, and which command families each one draws.
///
/// Generated rather than authored, deliberately. The hand-authored assignment
/// [docs/91](../docs/91-EXCEL-RIBBON-CHROME-DESIGN.md) §3 specifies is the next
/// slice; drawing these from the live command list first means the ribbon is
/// *complete* from the outset — every command the editor has is reachable in
/// it — rather than complete-looking with holes nobody has counted.
const TABS = [
  { id: "home", label: "Home" },
  { id: "insert", label: "Insert", from: ["insert"] },
  { id: "layout", label: "Page Layout", from: ["file.page", "view.freeze", "view.gridlines"] },
  { id: "formulas", label: "Formulas", from: ["format.number", "tools"] },
  { id: "data", label: "Data", from: ["data"] },
  { id: "review", label: "Review", from: ["tools", "help"] },
  { id: "view", label: "View", from: ["view"] },
];

const SKIP = new Set([
  // Region plumbing rather than commands: these are the toolbar's own overflow
  // machinery and the status line, none of which a ribbon has a place for.
  "toolbar.more", "toolbar.more-flyout", "toolbar.status",
  "toolbar.settings", "toolbar.open", "toolbar.delete-sheet",
  "toolbar.font-caret", "toolbar.size-caret", "toolbar.numfmt-label",
]);

function labelFor(node, id) {
  const raw = (node.dataset.ocLabel || node.getAttribute("title")
    || node.getAttribute("aria-label") || node.textContent || "").trim();
  // A tooltip is a sentence — "Bold (Ctrl+B)", "Format painter — click to…".
  // The ribbon wants the verb, so take the text before the first bracket or
  // dash and fall back to the id's last segment when that leaves nothing.
  const short = raw.split(/\s*[(—–-]\s/)[0].trim();
  return short || id.split(".").pop().replace(/-/g, " ");
}

/// A proxy for a command that has no relocatable control — a menu item.
///
/// It clones the source's icon when it has one, so the ribbon's iconography is
/// the editor's own rather than a second set that drifts. It carries no command
/// id (see the module comment) and it forwards rather than acts.
function proxy(node, id, size) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = size === "large" ? "rb-btn rb-large" : "rb-btn";
  b.dataset.rbFor = id;
  const svg = node.querySelector("svg");
  if (svg) b.appendChild(svg.cloneNode(true));
  const span = document.createElement("span");
  span.className = "rb-label";
  span.textContent = labelFor(node, id);
  b.appendChild(span);
  b.title = (node.getAttribute("title") || span.textContent);
  b.addEventListener("click", (e) => {
    e.preventDefault();
    // Click the real thing. Menu items are inside a closed dropdown, so they
    // are not visible — `click()` does not care, and the handler is the same
    // one the menu would have run.
    node.click();
  });
  return b;
}

/// Keep a proxy's enabled state honest.
///
/// `applyCommandRules()` disables the real node when a capability or read-only
/// mode forbids it. A proxy that stayed enabled would offer a command the
/// editor has already decided this user may not run — so it mirrors, and it
/// mirrors from the source rather than re-deriving the rule.
function syncProxies(root) {
  for (const b of root.querySelectorAll("[data-rb-for]")) {
    const src = byCommand(b.dataset.rbFor);
    if (!src) { b.disabled = true; continue; }
    b.disabled = !!src.disabled || src.classList.contains("oc-cmd-hidden");
    const pressed = src.getAttribute("aria-pressed");
    if (pressed !== null) b.setAttribute("aria-pressed", pressed);
  }
}

function buildGroup(spec, body, missing) {
  const g = document.createElement("div");
  g.className = "rb-group";
  const row = document.createElement("div");
  row.className = "rb-row";
  g.appendChild(row);
  let drawn = 0;
  for (const item of spec.items) {
    const node = byCommand(item.cmd);
    if (!node) { missing.push(item.cmd); continue; }
    if (node.closest(".toolbar")) {
      // A live control: take the node itself, so its behaviour comes with it.
      node.classList.add("rb-hosted");
      borrow(node, row);
    } else {
      row.appendChild(proxy(node, item.cmd, item.size));
    }
    drawn++;
  }
  // A group with nothing in it is not drawn. `applyCommandRules()` already
  // establishes the cascade for this: a capability that removes every control
  // in a group should remove the group, not leave a labelled empty box.
  if (!drawn) return null;
  const cap = document.createElement("div");
  cap.className = "rb-cap";
  cap.textContent = spec.label;
  g.appendChild(cap);
  body.appendChild(g);
  return g;
}

/// Everything under a set of id prefixes, bucketed into groups by family.
function generatedGroups(prefixes) {
  const seen = new Set();
  const buckets = new Map();
  for (const node of document.querySelectorAll("[data-oc-command]")) {
    const id = node.dataset.ocCommand;
    if (SKIP.has(id) || seen.has(id)) continue;
    if (!prefixes.some((p) => id === p || id.startsWith(p + "."))) continue;
    // A bare family id (`data`, `view`) is the menu's own button, not a command.
    if (!id.includes(".")) continue;
    // Menu items only. A toolbar control belongs to Home, which is authored.
    if (node.closest(".toolbar")) continue;
    seen.add(id);
    const parts = id.split(".");
    const family = parts.length > 2 ? parts.slice(0, 2).join(".") : parts[0];
    if (!buckets.has(family)) buckets.set(family, []);
    buckets.get(family).push({ cmd: id });
  }
  return [...buckets.entries()].map(([family, items]) => ({
    label: family.split(".").pop().replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase()),
    items,
  }));
}

let el = null;      // the ribbon root, while it exists
let borrowed = [];  // live controls currently on loan from the toolbar

export function ribbonRequested() {
  return (PARAMS.get("chrome") || "").toLowerCase() === "ribbon";
}

export function start() {
  if (el) return el;
  const anchor = document.querySelector(".formula-bar");
  if (!anchor) return null;

  el = document.createElement("div");
  el.className = "oc-ribbon";
  const strip = document.createElement("div");
  strip.className = "rb-tabs";
  strip.setAttribute("role", "tablist");
  strip.setAttribute("aria-label", "Ribbon");
  const body = document.createElement("div");
  body.className = "rb-body";
  el.append(strip, body);
  anchor.parentElement.insertBefore(el, anchor);

  const missing = [];
  const panels = new Map();

  for (const tab of TABS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "rb-tab";
    btn.setAttribute("role", "tab");
    btn.id = `rb-tab-${tab.id}`;
    btn.textContent = tab.label;
    strip.appendChild(btn);

    const panel = document.createElement("div");
    panel.className = "rb-panel";
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", btn.id);
    panel.hidden = true;
    body.appendChild(panel);
    panels.set(tab.id, panel);

    const groups = tab.id === "home" ? HOME : generatedGroups(tab.from);
    for (const spec of groups) buildGroup(spec, panel, missing);

    btn.addEventListener("click", () => select(tab.id));
  }

  // Arrow-key movement inside the strip, which `role="tablist"` promises.
  strip.addEventListener("keydown", (e) => {
    const tabs = [...strip.querySelectorAll(".rb-tab")];
    const i = tabs.indexOf(document.activeElement);
    if (i < 0) return;
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = tabs[(i + step + tabs.length) % tabs.length];
    next.focus();
    next.click();
  });

  function select(id) {
    for (const [k, p] of panels) p.hidden = k !== id;
    for (const b of strip.querySelectorAll(".rb-tab")) {
      const on = b.id === `rb-tab-${id}`;
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-selected", on ? "true" : "false");
      b.tabIndex = on ? 0 : -1;
    }
    syncProxies(el);
  }

  borrowed = [...el.querySelectorAll(".rb-hosted")];
  select("home");
  document.documentElement.classList.add("oc-chrome-ribbon");

  // Proxy state follows the editor's own state. `applyCommandRules()` and the
  // selection both change `disabled`/`aria-pressed` on the real nodes; watching
  // the document is cheaper than guessing when that happens, and it cannot go
  // out of date the way a list of hook points would.
  const obs = new MutationObserver(() => syncProxies(el));
  obs.observe(document.body, {
    subtree: true, attributes: true,
    attributeFilter: ["disabled", "aria-pressed", "class"],
  });
  el._obs = obs;

  if (missing.length) {
    // Counted, never silent. A command this file names and the editor does not
    // have is a defect in this file, and it says so rather than drawing a
    // button that does nothing.
    console.warn(`[ribbon] ${missing.length} command(s) named but not found:`, missing);
  }
  return el;
}

export function stop() {
  if (!el) return;
  el._obs?.disconnect();
  for (const node of borrowed) { node.classList.remove("rb-hosted"); giveBack(node); }
  borrowed = [];
  el.remove();
  el = null;
  document.documentElement.classList.remove("oc-chrome-ribbon");
}

export function toggle(on) { on ? start() : stop(); }
