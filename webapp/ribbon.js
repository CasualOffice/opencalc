/// The ribbon chrome (`UX-RIB-04`, [docs/91](../docs/91-EXCEL-RIBBON-CHROME-DESIGN.md)).
///
/// **This module draws a ribbon. It does not implement a single command.**
///
/// That is the whole design, and it is why this file is as short as it is.
/// Every verb the editor has already exists as a node carrying
/// `data-oc-command`, already wired, already gated by `applyCommandRules()`,
/// already counted by `listCommands()`. A ribbon that re-implemented Bold would
/// be a second Bold that drifts from the first; a ribbon that re-implemented
/// Freeze would be a worse Freeze. So this does neither:
///
/// - **A live toolbar control is *moved* into the ribbon**, and moved back when
///   the ribbon goes away. It keeps its listeners, its state mirroring and its
///   identity, because it is the same node — the pattern `applyModeChrome()`
///   already uses for desktop chrome, and for the same reason.
/// - **Everything else gets a proxy** that clicks the owning node. §3.0 R2
///   names this as the assignment's own mechanism, and gives it a spelling:
///   `data-oc-proxy`, never a second `data-oc-command`. ADR-026 fixes ids as
///   slugs of the English menu path and both the capability table and the
///   read-only whitelist match them **by regex**, so minting `ribbon.bold`
///   beside `toolbar.bold` would leave a viewer-mode editor able to save.
///
/// The consequence worth stating plainly: nothing below the chrome changes.
/// Freeze still drags, the header context menu still re-targets, undo still
/// goes through the transaction layer — because none of it was touched.
///
/// **Every tab is in the DOM at all times** (§3.0 R1). `listCommands()` reads
/// the live DOM, so rendering only the active tab would silently shrink the
/// SDK's published surface each time the user clicked a tab. Inactive panels
/// hide with CSS.

import { ASSIGN, item } from "./ribbon.model.js?v=3";

const PARAMS = new URL(location.href).searchParams;
const STORE_KEY = "oc.chrome";
const LAYOUT_KEY = "oc.ribbon.layout";

/* ── The preference ───────────────────────────────────────────────────────── */

/// Which chrome to draw, and who decides.
///
/// Precedence, highest first, and each level exists for a reason:
///   1. `?chrome=` — an explicit request on the URL, which is how the test
///      harnesses and a linked demo ask for one.
///   2. the stored user preference — the product owner's requirement, and the
///      only level a person sets by clicking something.
///   3. the per-mount default. `?mode=desktop` / `?chrome=native` means the
///      desktop shell, where `docs/91` makes the ribbon the default; a browser
///      tab keeps the toolbar until asked.
export function chosenChrome() {
  const asked = (PARAMS.get("chrome") || "").toLowerCase();
  if (asked === "ribbon" || asked === "toolbar") return asked;
  let stored = null;
  try { stored = localStorage.getItem(STORE_KEY); } catch { /* private mode */ }
  if (stored === "ribbon" || stored === "toolbar") return stored;
  const native = asked === "native" || (PARAMS.get("mode") || "") === "desktop";
  return native ? "ribbon" : "toolbar";
}

export function setChrome(which) {
  try { localStorage.setItem(STORE_KEY, which); } catch { /* private mode */ }
  toggle(which === "ribbon");
}

function storedLayout() {
  try { return localStorage.getItem(LAYOUT_KEY) || "classic"; } catch { return "classic"; }
}
function storeLayout(v) { try { localStorage.setItem(LAYOUT_KEY, v); } catch { /* ignore */ } }

/* ── Borrowing live controls ──────────────────────────────────────────────── */

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
  // **The recorded sibling may no longer be a sibling**, and this threw.
  //
  // Each borrowed node records the `next` it had at the moment it was taken.
  // Twenty-four of them come out of the same `.toolbar`, so by the time the
  // last is recorded, several of the recorded `next` nodes are themselves
  // sitting in the ribbon. Restoring then calls `insertBefore(node, next)` with
  // a reference that is not a child of `parent` any more, which is a
  // `NotFoundError` — and because it threw mid-loop, `stop()` never reached
  // `el.remove()`. The symptom was the switch appearing to do nothing while
  // leaving the toolbar empty: the ribbon still up, its controls in neither
  // place. Falling back to `null` appends, which is right, and `stop()`
  // restores in reverse so the appends rebuild the original order.
  const ref = h.next && h.next.parentElement === h.parent ? h.next : null;
  h.parent.insertBefore(node, ref);
}

const byCommand = (id) => document.querySelector(`[data-oc-command="${CSS.escape(id)}"]`);

/* ── Labels ───────────────────────────────────────────────────────────────── */

/// Excel's word, then ours, then the id.
///
/// §3.0 R4: where our label reads better the ribbon still uses Excel's, because
/// the user this round is for is the one arriving from Excel — and our better
/// word survives in the tooltip, which is where this takes it from.
const EXCEL_WORDS = {
  "data.filter": "Filter",
  "toolbar.filter": "Filter",
  "view.cell-markings": "Headings",
  "toolbar.merge": "Merge & Center",
  "toolbar.wrap": "Wrap Text",
  "toolbar.numfmt": "Number Format",
  "toolbar.currency": "Accounting",
  "toolbar.painter": "Format Painter",
  "edit.find-replace": "Find & Select",
  "data.sort-range": "Sort & Filter",
  "format.conditional-formatting": "Conditional Formatting",
  "format.cell-styles": "Cell Styles",
  "toolbar.freeze": "Freeze Panes",
};

function labelFor(node, id) {
  if (EXCEL_WORDS[id]) return EXCEL_WORDS[id];
  const raw = (node.dataset.ocLabel || node.getAttribute("title")
    || node.getAttribute("aria-label") || node.textContent || "").trim();
  // A tooltip is a sentence — "Bold (Ctrl+B)", "Format painter — click to…".
  // The ribbon wants the verb, so take the text before the first bracket or
  // dash and fall back to the id's last segment when that leaves nothing.
  const short = raw.split(/\s*[(—–]|\s+-\s+/)[0].trim();
  return short || id.split(".").pop().replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase());
}

/* ── Controls ─────────────────────────────────────────────────────────────── */

/// A proxy for a command whose owning node cannot be relocated — a menu item,
/// or the second slot where Excel draws one verb twice (§3.0 R2).
///
/// It clones the source's icon when it has one, so the ribbon's iconography is
/// the editor's own rather than a second set that drifts. It carries
/// `data-oc-proxy` and never `data-oc-command`, and it forwards rather than acts.
function proxy(node, id, size) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = `rb-btn rb-${size}`;
  b.dataset.ocProxy = id;
  const svg = node.querySelector("svg");
  if (svg) b.appendChild(svg.cloneNode(true));
  const span = document.createElement("span");
  span.className = "rb-label";
  span.textContent = labelFor(node, id);
  b.appendChild(span);
  b.title = node.getAttribute("title") || span.textContent;
  b.addEventListener("click", (e) => {
    e.preventDefault();
    // Click the real thing. Menu items sit inside a closed dropdown so they are
    // not visible — `click()` does not care, and the handler is the one the
    // menu would have run.
    node.click();
  });
  return b;
}

/// Keep proxies honest about state.
///
/// `applyCommandRules()` disables the owning node when a capability or
/// read-only mode forbids it, and toggles carry `aria-pressed`. A proxy that
/// stayed enabled would offer a command the editor has already decided this
/// user may not run, so it mirrors — from the source, never re-deriving the rule.
function syncProxies(root) {
  for (const b of root.querySelectorAll("[data-oc-proxy]")) {
    const src = byCommand(b.dataset.ocProxy);
    if (!src) { b.disabled = true; continue; }
    b.disabled = !!src.disabled || src.classList.contains("oc-cmd-hidden");
    const pressed = src.getAttribute("aria-pressed");
    if (pressed !== null) b.setAttribute("aria-pressed", pressed);
  }
}

/* ── Build ────────────────────────────────────────────────────────────────── */

let el = null;
let borrowed = [];
let onKey = null;

/// The Backstage (§6). Not a ribbon tab: Excel's File takes the whole window.
///
/// Built from the `file.*` commands the editor actually has, in §3.2's rail
/// order. A rail entry whose commands are all absent is not drawn — the same
/// rule as an empty group, for the same reason.
const RAIL = [
  { label: "New", cmds: ["file.new"] },
  { label: "Open", cmds: ["file.open"] },
  { label: "Save", cmds: ["file.save", "file.download"] },
  { label: "Print", cmds: ["file.print", "file.export-as-pdf", "file.page-setup", "file.page-break-here"] },
  { label: "Share", cmds: ["file.share", "file.version-history"] },
  { label: "Info", cmds: ["file.properties"] },
];

function buildBackstage() {
  const bs = document.createElement("div");
  bs.className = "rb-backstage";
  bs.hidden = true;
  bs.setAttribute("role", "dialog");
  bs.setAttribute("aria-label", "File");

  const back = document.createElement("button");
  back.type = "button";
  back.className = "rb-back";
  back.setAttribute("aria-label", "Back");
  back.textContent = "←";
  back.addEventListener("click", () => closeBackstage());

  const rail = document.createElement("div");
  rail.className = "rb-rail";
  const pane = document.createElement("div");
  pane.className = "rb-pane";
  bs.append(back, rail, pane);

  const panes = [];
  for (const entry of RAIL) {
    const live = entry.cmds.map((c) => [c, byCommand(c)]).filter(([, n]) => n);
    if (!live.length) continue;
    const r = document.createElement("button");
    r.type = "button";
    r.className = "rb-rail-item";
    r.textContent = entry.label;
    const p = document.createElement("div");
    p.className = "rb-pane-body";
    p.hidden = true;
    const h = document.createElement("h2");
    h.className = "rb-pane-title";
    h.textContent = entry.label;
    p.appendChild(h);
    for (const [id, node] of live) p.appendChild(proxy(node, id, "large"));
    pane.appendChild(p);
    rail.appendChild(r);
    panes.push([r, p]);
    r.addEventListener("click", () => {
      for (const [rr, pp] of panes) { pp.hidden = rr !== r; rr.classList.toggle("is-on", rr === r); }
    });
  }
  if (panes.length) panes[0][0].click();
  return bs;
}

function openBackstage() {
  const bs = el?.querySelector(".rb-backstage");
  if (!bs) return;
  bs.hidden = false;
  document.documentElement.classList.add("oc-backstage-open");
  bs.querySelector(".rb-rail-item")?.focus();
}
function closeBackstage() {
  const bs = el?.querySelector(".rb-backstage");
  if (!bs) return;
  bs.hidden = true;
  document.documentElement.classList.remove("oc-backstage-open");
  el?.querySelector(".rb-tab.is-on")?.focus();
}

function buildGroup(spec, panel, missing) {
  const g = document.createElement("div");
  g.className = "rb-group";
  g.dataset.ord = String(spec.ord ?? 99);
  const row = document.createElement("div");
  row.className = "rb-row";
  g.appendChild(row);
  let drawn = 0;
  for (const raw of spec.items) {
    const { cmd, size } = item(raw);
    const node = byCommand(cmd);
    if (!node) { missing.push(cmd); continue; }
    if (node.closest(".toolbar")) {
      node.classList.add("rb-hosted");
      borrow(node, row);
    } else {
      row.appendChild(proxy(node, cmd, size));
    }
    drawn++;
  }
  // A group with nothing in it is not drawn. `applyCommandRules()` already
  // establishes this cascade: a capability that removes every control in a
  // group should remove the group, not leave a labelled empty box.
  if (!drawn) return;
  const cap = document.createElement("div");
  cap.className = "rb-cap";
  cap.textContent = spec.label;
  g.appendChild(cap);
  panel.appendChild(g);
}

/// KeyTips: Alt reveals a letter on the File button and every tab (§7).
///
/// A demonstration of the scheme rather than Excel's full two-level tree — the
/// second level is per-control and belongs with the authored per-control
/// letters, which §7 assigns and this slice does not yet draw.
const TIPS = ["F", "H", "N", "P", "M", "A", "R", "W", "Y"];

function wireKeyTips(strip, fileBtn) {
  const show = (on) => {
    el.classList.toggle("rb-keytips", on);
    const targets = [fileBtn, ...strip.querySelectorAll(".rb-tab")];
    targets.forEach((t, i) => {
      let tip = t.querySelector(".rb-tip");
      if (on && !tip) {
        tip = document.createElement("span");
        tip.className = "rb-tip";
        tip.textContent = TIPS[i] || "";
        t.appendChild(tip);
      } else if (!on && tip) tip.remove();
    });
  };
  onKey = (e) => {
    if (e.key === "Alt" && !e.repeat) { show(true); return; }
    if (e.key === "Escape") { show(false); closeBackstage(); return; }
    if (!el.classList.contains("rb-keytips")) return;
    const i = TIPS.indexOf(e.key.toUpperCase());
    if (i < 0) return;
    e.preventDefault();
    show(false);
    const targets = [fileBtn, ...strip.querySelectorAll(".rb-tab")];
    targets[i]?.click();
  };
  document.addEventListener("keydown", onKey);
  document.addEventListener("keyup", (e) => { if (e.key === "Alt") { /* keep shown */ } });
}

export function ribbonRequested() { return chosenChrome() === "ribbon"; }

export function start() {
  if (el) return el;
  const anchor = document.querySelector(".formula-bar");
  if (!anchor) return null;

  el = document.createElement("div");
  el.className = "oc-ribbon";

  const strip = document.createElement("div");
  strip.className = "rb-tabs";

  // The File button is not a tab and must not be in the tablist: it opens a
  // dialog rather than selecting a panel, and announcing it as a tab would
  // promise a tabpanel that does not exist.
  const fileBtn = document.createElement("button");
  fileBtn.type = "button";
  fileBtn.className = "rb-file";
  fileBtn.textContent = "File";
  fileBtn.setAttribute("aria-haspopup", "dialog");
  fileBtn.addEventListener("click", () => openBackstage());

  const tablist = document.createElement("div");
  tablist.className = "rb-tablist";
  tablist.setAttribute("role", "tablist");
  tablist.setAttribute("aria-label", "Ribbon");

  // Layout switch: Classic ⇄ Simplified. `docs/91` §4 makes Simplified the
  // default *shape*; which one a person last chose is theirs and persists.
  const layoutBtn = document.createElement("button");
  layoutBtn.type = "button";
  layoutBtn.className = "rb-layout";
  layoutBtn.title = "Ribbon layout";
  layoutBtn.setAttribute("aria-label", "Ribbon layout");

  strip.append(fileBtn, tablist, layoutBtn);

  const body = document.createElement("div");
  body.className = "rb-body";
  el.append(strip, body);
  anchor.parentElement.insertBefore(el, anchor);

  const missing = [];
  const panels = new Map();

  for (const tab of ASSIGN) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "rb-tab";
    btn.setAttribute("role", "tab");
    btn.id = `rb-tab-${tab.id}`;
    btn.textContent = tab.label;
    tablist.appendChild(btn);

    const panel = document.createElement("div");
    panel.className = "rb-panel";
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", btn.id);
    panel.hidden = true;
    body.appendChild(panel);
    panels.set(tab.id, panel);

    // **Draw order is the model's order. `ord` is the *collapse* order.**
    //
    // These are two different things and sorting by `ord` conflated them: §3.0
    // R6 assigns Clipboard 6, Styles 5, Cells 4, so drawing in `ord` order put
    // Cells first and Clipboard fifth, and Home opened on the wrong group
    // entirely. Excel's draw order is Clipboard, Font, Alignment, Number,
    // Styles, Cells, Editing — which is the order §3.3's table already lists,
    // so the model carries it and this must not re-sort. `ord` is read by the
    // collapse pass, which is where "lowest collapses first" belongs.
    for (const spec of tab.groups) buildGroup(spec, panel, missing);
    btn.addEventListener("click", () => select(tab.id));
  }

  el.appendChild(buildBackstage());

  tablist.addEventListener("keydown", (e) => {
    const tabs = [...tablist.querySelectorAll(".rb-tab")];
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
    for (const b of tablist.querySelectorAll(".rb-tab")) {
      const on = b.id === `rb-tab-${id}`;
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-selected", on ? "true" : "false");
      b.tabIndex = on ? 0 : -1;
    }
    syncProxies(el);
  }

  function applyLayout(v) {
    el.classList.toggle("rb-simplified", v === "simplified");
    layoutBtn.textContent = v === "simplified" ? "⌃" : "⌄";
    layoutBtn.title = v === "simplified" ? "Classic ribbon" : "Simplified ribbon";
    storeLayout(v);
  }
  layoutBtn.addEventListener("click", () => {
    applyLayout(el.classList.contains("rb-simplified") ? "classic" : "simplified");
  });

  borrowed = [...el.querySelectorAll(".rb-hosted")];
  select(ASSIGN[0].id);
  applyLayout(storedLayout());
  document.documentElement.classList.add("oc-chrome-ribbon");
  wireKeyTips(tablist, fileBtn);

  // Proxy state follows the editor's own. `applyCommandRules()` and the
  // selection both change `disabled`/`aria-pressed` on the owning nodes;
  // watching the document is cheaper than guessing when that happens, and it
  // cannot go stale the way a list of hook points would.
  //
  // **Two guards, and the first one is not optional.** `syncProxies()` writes
  // `disabled` and `aria-pressed` onto the proxies, the proxies are inside
  // `document.body`, and those writes are exactly the attributes being watched
  // — so the naive form feeds itself and the renderer locks up. It did: this
  // froze the tab hard enough that the debugger timed out rather than
  // returning. Mutations originating inside the ribbon are therefore ignored,
  // and the pass is coalesced to one per frame so a burst of edits costs one
  // sync rather than hundreds.
  let queued = false;
  const obs = new MutationObserver((records) => {
    if (queued) return;
    if (!records.some((r) => r.target instanceof Element && !el.contains(r.target))) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; syncProxies(el); });
  });
  obs.observe(document.body, {
    subtree: true, attributes: true,
    attributeFilter: ["disabled", "aria-pressed", "class"],
  });
  el._obs = obs;

  if (missing.length) {
    // Counted, never silent. A command the model names and this build does not
    // have is a defect in the model, and it says so rather than drawing a
    // button that does nothing.
    console.warn(`[ribbon] ${missing.length} of ${missing.length + document.querySelectorAll(".oc-ribbon [data-oc-proxy],.oc-ribbon .rb-hosted").length} assigned commands are absent from this build:`, missing);
  }
  return el;
}

export function stop() {
  if (!el) return;
  el._obs?.disconnect();
  if (onKey) { document.removeEventListener("keydown", onKey); onKey = null; }
  // Reverse, so that the append fallback in `giveBack()` rebuilds the original
  // order rather than reversing it. And in a `try` apiece: one node that cannot
  // be placed must not strand the other twenty-three in the ribbon, which is
  // the failure this whole comment is about.
  for (const node of [...borrowed].reverse()) {
    node.classList.remove("rb-hosted");
    try { giveBack(node); } catch (e) { console.warn("[ribbon] could not restore", node.id, e); }
  }
  borrowed = [];
  el.remove();
  el = null;
  document.documentElement.classList.remove("oc-chrome-ribbon", "oc-backstage-open");
}

export function toggle(on) { on ? start() : stop(); }
