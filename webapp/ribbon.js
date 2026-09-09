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

import { ASSIGN, item } from "./ribbon.model.js?v=4";
import { iconIds } from "./ribbon.icons.js?v=2";

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
const CHROMES = ["toolbar", "ribbon", "sheets"];

export function chosenChrome() {
  const asked = (PARAMS.get("chrome") || "").toLowerCase();
  if (CHROMES.includes(asked)) return asked;
  let stored = null;
  try { stored = localStorage.getItem(STORE_KEY); } catch { /* private mode */ }
  if (CHROMES.includes(stored)) return stored;
  const native = asked === "native" || (PARAMS.get("mode") || "") === "desktop";
  return native ? "ribbon" : "toolbar";
}

/// Switching is the caller's job to sequence, because the two chromes borrow
/// from the same toolbar: whichever is up must give its controls back *before*
/// the other takes them, or the second finds an empty toolbar and draws a row
/// of nothing. `editor.html` owns that order; this only records the choice.
export function setChrome(which) {
  try { localStorage.setItem(STORE_KEY, which); } catch { /* private mode */ }
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

/// Region plumbing rather than commands.
///
/// The toolbar's own overflow machinery, the status line and the two hidden
/// picker nodes. None is a verb a person would look for, so none counts as
/// unreachable when the ribbon does not draw it. (This constant existed for the
/// generated-tabs version, went with it, and was then referenced by
/// `unplacedCommands()` — a `ReferenceError` that took the whole backstage down
/// with it, silently, because `start()` is awaited and nothing was watching.)
const SKIP = new Set([
  "toolbar.more", "toolbar.more-flyout", "toolbar.status",
  "toolbar.settings", "toolbar.open", "toolbar.delete-sheet",
  "toolbar.font-caret", "toolbar.size-caret", "toolbar.numfmt-label",
]);

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
  // `.mi-label` first: a menu item wraps its text in one, beside a `.mi-check`
  // spacer, so `textContent` picks up the checkmark slot as well.
  const raw = (node.querySelector(".mi-label")?.textContent
    || node.dataset.ocLabel || node.getAttribute("title")
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
  const large = size === "large";
  const b = document.createElement("button");
  b.type = "button";
  b.className = `rb-btn rb-${size}`;
  b.dataset.ocProxy = id;
  b._src = node;   // the owning node, kept so `syncProxies` need not re-query

  // **The icon comes from the sprite, not from the source node.**
  //
  // The first version cloned the source's `<svg>`, which is right for a toolbar
  // control and useless for a menu item: this editor's menus carry no icons at
  // all, so 99 of 99 proxies found nothing and rendered as text. That is the
  // difference between the ribbon reading as Excel and reading as a list of
  // words, and it was the whole of it.
  const sym = iconIds(id, large).find((c) => document.getElementById(c));
  if (sym) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
    use.setAttribute("href", `#${sym}`);
    svg.appendChild(use);
    b.appendChild(svg);
  } else {
    // No icon rather than a wrong one: a glyph that means something else
    // teaches the wrong thing, and the label still says what this does.
    b.classList.add("rb-noicon");
  }

  const text = labelFor(node, id);
  // **Excel labels the large controls and lets the small ones be icons.** A
  // group of ten labelled buttons is a menu drawn sideways; the label lives in
  // the tooltip, where it is one hover away and costs no width. Small controls
  // that have no icon keep their text, because an unlabelled blank is worse
  // than a wide one.
  // **A list always shows its words.** In a ribbon group an icon alone is
  // right — Excel does it, and the tooltip carries the name. In a *pane* it is
  // not: a column of forty unlabelled glyphs is unreadable, and that is exactly
  // what "All commands" rendered the first time. So `list` forces the label.
  if (large || size === "list" || !sym) {
    const span = document.createElement("span");
    span.className = "rb-label";
    span.textContent = text;
    b.appendChild(span);
  }
  b.setAttribute("aria-label", text);
  const tip = node.getAttribute("title");
  b.title = tip && tip.length > text.length ? tip : text;

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
    const src = b._src || byCommand(b.dataset.ocProxy);
    if (!src) { b.disabled = true; continue; }
    b.disabled = !!src.disabled || src.classList.contains("oc-cmd-hidden");
    const pressed = src.getAttribute("aria-pressed");
    if (pressed !== null) b.setAttribute("aria-pressed", pressed);
  }
}

/* ── Build ────────────────────────────────────────────────────────────────── */

let el = null;
let backstage = null;
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

/// Every command the ribbon did not place.
///
/// **The ribbon hides the menu bar, so a command it does not draw is a command
/// nobody can click.** `docs/91` §3 assigns 147 controls and this build has 205
/// commands, so without this the ribbon would quietly cost a user roughly
/// eighty verbs the toolbar chrome gives them — the exact "nothing should be
/// removed" failure, and one that would have been invisible until somebody went
/// looking for a menu item that no longer existed.
///
/// They go in the backstage rather than into a tab, because they are the long
/// tail by definition: if one turns out to be used daily it has earned a place
/// in `docs/91` §3, and this list is the evidence for that argument.
function unplacedCommands(root) {
  const placed = new Set();
  for (const n of root.querySelectorAll("[data-oc-command]")) placed.add(n.dataset.ocCommand);
  for (const n of root.querySelectorAll("[data-oc-proxy]")) placed.add(n.dataset.ocProxy);
  const out = [];
  for (const n of document.querySelectorAll("[data-oc-command]")) {
    const id = n.dataset.ocCommand;
    if (placed.has(id) || SKIP.has(id)) continue;
    // A bare family id (`data`, `view`) is the menu's own button, not a verb.
    if (!id.includes(".")) continue;
    if (n.closest(".oc-ribbon")) continue;
    placed.add(id);
    out.push([id, n]);
  }
  return out;
}

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
    // Each listed id **and everything beneath it**. `file.download` is a
    // submenu whose children are generated from the formats the engine can
    // write, so naming the parent alone gave a "Save" pane with one button in
    // it and no way to reach any format.
    const seen = new Set();
    const live = [];
    for (const c of entry.cmds) {
      for (const n of document.querySelectorAll("[data-oc-command]")) {
        const id = n.dataset.ocCommand;
        if (id !== c && !id.startsWith(c + ".")) continue;
        if (seen.has(id) || SKIP.has(id)) continue;
        seen.add(id);
        live.push([id, n]);
      }
    }
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
    for (const [id, node] of live) p.appendChild(proxy(node, id, "list"));
    pane.appendChild(p);
    rail.appendChild(r);
    panes.push([r, p]);
    r.addEventListener("click", () => {
      for (const [rr, pp] of panes) { pp.hidden = rr !== r; rr.classList.toggle("is-on", rr === r); }
    });
  }
  // The long tail, grouped by the menu it came from so it stays findable.
  const rest = unplacedCommands(el);
  if (rest.length) {
    const r = document.createElement("button");
    r.type = "button";
    r.className = "rb-rail-item";
    r.textContent = "All commands";
    const p = document.createElement("div");
    p.className = "rb-pane-body rb-pane-all";
    p.hidden = true;
    const h = document.createElement("h2");
    h.className = "rb-pane-title";
    h.textContent = "All commands";
    const note = document.createElement("p");
    note.className = "rb-pane-note";
    note.textContent = `${rest.length} commands that do not have a place on a tab. Everything the editor can do is here.`;
    p.append(h, note);
    const byFamily = new Map();
    for (const [id, node] of rest) {
      const fam = id.split(".")[0];
      if (!byFamily.has(fam)) byFamily.set(fam, []);
      byFamily.get(fam).push([id, node]);
    }
    for (const [fam, items] of byFamily) {
      const sec = document.createElement("div");
      sec.className = "rb-all-group";
      const cap = document.createElement("h3");
      cap.textContent = fam.replace(/^./, (c) => c.toUpperCase());
      sec.appendChild(cap);
      const wrap = document.createElement("div");
      wrap.className = "rb-all-items";
      for (const [id, node] of items) wrap.appendChild(proxy(node, id, "list"));
      sec.appendChild(wrap);
      p.appendChild(sec);
    }
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
  const bs = backstage;
  if (!bs) return;
  bs.hidden = false;
  document.documentElement.classList.add("oc-backstage-open");
  bs.querySelector(".rb-rail-item")?.focus();
}
function closeBackstage() {
  const bs = backstage;
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
      // **Make it reachable.** The toolbar uses a roving `tabindex`, so every
      // control but one carries `-1` — correct there, because the toolbar
      // handles arrow keys, and a hard WCAG 2.1.1 failure here, because nothing
      // in this chrome does. Measured: 23 of the visible band's controls could
      // not be reached by keyboard at all. The original is remembered and put
      // back by `stop()`, so the toolbar keeps its own model.
      if (node.dataset.rbTabindex === undefined) {
        node.dataset.rbTabindex = node.hasAttribute("tabindex") ? node.getAttribute("tabindex") : "";
      }
      node.tabIndex = 0;
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

/// The icon sprite, fetched once and parked in the document.
///
/// A `<use href="#id">` resolves against the *current document*, so the symbols
/// have to be in it — a stylesheet cannot carry them and an `<img>` cannot be
/// referenced this way. Fetched rather than inlined into `editor.html` because
/// it is 99KB that the default chrome has no use for.
let spritePromise = null;
function ensureSprite() {
  if (spritePromise || document.getElementById("oc-ribbon-icons")) return spritePromise;
  spritePromise = fetch(new URL("./ribbon.icons.svg", import.meta.url))
    .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`sprite ${r.status}`))))
    .then((svg) => {
      if (document.getElementById("oc-ribbon-icons")) return;
      const host = document.createElement("div");
      host.style.display = "none";
      host.innerHTML = svg;
      document.body.insertBefore(host.firstElementChild, document.body.firstChild);
    })
    .catch((e) => {
      // Every control still has its label and its tooltip, so this degrades to
      // the text form rather than to blank buttons. Say so once.
      console.warn("[ribbon] icon sprite failed to load; drawing labels only", e);
    });
  return spritePromise;
}

export function ribbonRequested() { return chosenChrome() === "ribbon"; }

export async function start() {
  if (el) return el;
  // **Await the sprite before building anything.** `proxy()` picks its icon by
  // asking whether the symbol exists, and the sprite arrives over `fetch` — so
  // building first meant every lookup missed and all 99 proxies fell back to
  // text. The symptom was identical to having no icon map at all, which is why
  // it is worth naming: an async dependency consumed synchronously fails
  // *silently and completely*.
  await ensureSprite();
  const anchor = document.querySelector(".formula-bar");
  if (!anchor) return null;

  ensureSprite();

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
  const tabPanels = [];

  // Which id families each tab owns, for the overflow pass below. Ordered:
  // the first tab that claims a family gets it.
  const FAMILIES = {
    home: ["format.", "edit.", "toolbar."],
    insert: ["insert.", "table."],
    pagelayout: ["file.page", "view.gridlines", "view.cell-markings", "view.zoom"],
    formulas: ["formulas.", "tools.calculation", "format.trace"],
    data: ["data.", "pivot."],
    review: ["format.protection", "insert.note", "tools."],
    view: ["view."],
    help: ["help."],
  };

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

    // Everything §3 did not place, routed to the tab it belongs on.
    //
    // §3 assigns 147 controls and 35 of them do not exist under those ids in
    // this build, which left Help with one control, Page Layout with four and
    // Review with four — tabs that read as broken rather than as sparse, while
    // 82 real commands sat in a single catch-all list. A command's id already
    // says where it belongs (`data.*` is the Data tab's, `view.*` the View
    // tab's), so it is placed there under a "More" caption rather than exiled.
    // "All commands" then holds a genuine remainder instead of most of the
    // editor.
    //
    // Authored placement always wins: this only ever sees what §3 left over.

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
    tabPanels.push([tab, panel]);
    btn.addEventListener("click", () => select(tab.id));
  }

  // Place the leftovers before the backstage is built, so "All commands" sees
  // what is genuinely still unplaced rather than everything.
  {
    const claimed = new Set();
    for (const [tab, panel] of tabPanels) {
      const prefixes = FAMILIES[tab.id] || [];
      if (!prefixes.length) continue;
      const items = [];
      for (const [id, node] of unplacedCommands(el)) {
        if (claimed.has(id)) continue;
        if (!prefixes.some((f) => id.startsWith(f))) continue;
        claimed.add(id);
        items.push(id);
      }
      if (!items.length) continue;
      buildGroup({ label: "More", ord: 0, items }, panel, missing);
    }
  }

  // **On `document.body`, not inside the ribbon.**
  //
  // It was a child of `.oc-ribbon`, and a `position: fixed` element with
  // `z-index: 40` only outranks what shares its stacking context — so the grid
  // and the ribbon itself painted straight over it and the backstage rendered
  // see-through, with cell borders and tab labels showing through the pane. It
  // covers the window, so it belongs at the top level where nothing can be
  // above it. `stop()` removes it.
  //
  // After the tabs, so `unplacedCommands()` can see what they placed.
  backstage = buildBackstage();
  document.body.appendChild(backstage);

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

  /// **Collapse in the authored order until the panel fits** (`docs/91` §5).
  ///
  /// Home needs 1744px and a 1563px window has 1563 of them, so without this the
  /// last two groups simply run off the right-hand edge — which is the defect
  /// `docs/88` §1 measured on the old toolbar, reappearing in a wider bar.
  /// Groups fold lowest `ord` first, each becoming a button that drops its own
  /// contents; the order is authored per tab and never computed, because a
  /// computed order changes under the user as content changes.
  function fitPanel(panel) {
    for (const g of panel.querySelectorAll(".rb-group.is-folded")) unfold(g);
    const groups = [...panel.querySelectorAll(".rb-group")]
      .sort((a, b) => Number(a.dataset.ord) - Number(b.dataset.ord));
    for (const g of groups) {
      if (panel.scrollWidth <= panel.clientWidth) break;
      fold(g);
    }
  }

  function fold(g) {
    if (g.classList.contains("is-folded")) return;
    const cap = g.querySelector(".rb-cap")?.textContent || "";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "rb-folded";
    btn.setAttribute("aria-haspopup", "true");
    btn.setAttribute("aria-expanded", "false");
    btn.innerHTML = `<span>${cap}</span>`;
    btn.title = cap;
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = g.classList.toggle("is-open");
      btn.setAttribute("aria-expanded", open ? "true" : "false");
    });
    g.classList.add("is-folded");
    g.insertBefore(btn, g.firstChild);
  }

  function unfold(g) {
    g.classList.remove("is-folded", "is-open");
    g.querySelector(".rb-folded")?.remove();
  }

  function select(id) {
    for (const [k, p] of panels) p.hidden = k !== id;
    for (const b of tablist.querySelectorAll(".rb-tab")) {
      const on = b.id === `rb-tab-${id}`;
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-selected", on ? "true" : "false");
      b.tabIndex = on ? 0 : -1;
    }
    syncProxies(el);
    const live = panels.get(id);
    if (live) fitPanel(live);
  }

  function applyLayout(v) {
    el.classList.toggle("rb-simplified", v === "simplified");
    layoutBtn.textContent = v === "simplified" ? "⌃" : "⌄";
    layoutBtn.title = v === "simplified" ? "Classic ribbon" : "Simplified ribbon";
    storeLayout(v);
  }
  layoutBtn.addEventListener("click", () => {
    applyLayout(el.classList.contains("rb-simplified") ? "classic" : "simplified");
    const open = [...panels.values()].find((p) => !p.hidden);
    if (open) fitPanel(open);
  });

  // One reflow per frame on resize — the same coalescing the proxy sync uses,
  // and for the same reason: a drag-resize fires this continuously.
  let fitQueued = false;
  const onResize = () => {
    if (fitQueued) return;
    fitQueued = true;
    requestAnimationFrame(() => {
      fitQueued = false;
      const open = [...panels.values()].find((p) => !p.hidden);
      if (open) fitPanel(open);
    });
  };
  window.addEventListener("resize", onResize);
  el._onResize = onResize;

  borrowed = [...el.querySelectorAll(".rb-hosted")];
  // The class first: every metric in `ribbon.css` is scoped to it, so a panel
  // measured before it is applied has no constrained width and `fitPanel()`
  // concludes that everything fits. It folded nothing, at 2562px into 1563.
  document.documentElement.classList.add("oc-chrome-ribbon");
  applyLayout(storedLayout());
  select(ASSIGN[0].id);
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
  // **Watch the owning nodes, not the document.**
  //
  // The first version observed `document.body` with `subtree: true` and then
  // re-queried every proxy's source by id on each pass — 99 `querySelector`
  // calls answering every attribute change anywhere in the editor, including
  // the ones `syncProxies` itself had just made. It coalesced per frame and was
  // still enough to wedge the tab on a chrome switch.
  //
  // There are only ever ~100 nodes whose state a proxy mirrors, and they are
  // known at build time, so each is observed directly: no subtree, no
  // re-querying, and self-inflicted mutations are structurally impossible
  // because nothing here writes to a source node.
  let queued = false;
  const obs = new MutationObserver(() => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; syncProxies(el); });
  });
  for (const b of el.querySelectorAll("[data-oc-proxy]")) {
    if (b._src) obs.observe(b._src, { attributes: true, attributeFilter: ["disabled", "aria-pressed", "class"] });
  }
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
  if (el._onResize) window.removeEventListener("resize", el._onResize);
  if (onKey) { document.removeEventListener("keydown", onKey); onKey = null; }
  // Reverse, so that the append fallback in `giveBack()` rebuilds the original
  // order rather than reversing it. And in a `try` apiece: one node that cannot
  // be placed must not strand the other twenty-three in the ribbon, which is
  // the failure this whole comment is about.
  for (const node of [...borrowed].reverse()) {
    node.classList.remove("rb-hosted");
    // Put the toolbar's roving model back exactly as it was.
    if (node.dataset.rbTabindex !== undefined) {
      if (node.dataset.rbTabindex === "") node.removeAttribute("tabindex");
      else node.setAttribute("tabindex", node.dataset.rbTabindex);
      delete node.dataset.rbTabindex;
    }
    try { giveBack(node); } catch (e) { console.warn("[ribbon] could not restore", node.id, e); }
  }
  borrowed = [];
  backstage?.remove();
  backstage = null;
  el.remove();
  el = null;
  document.documentElement.classList.remove("oc-chrome-ribbon", "oc-backstage-open");
}

export function toggle(on) { on ? start() : stop(); }
