/// The Sheets chrome (`UX-GS-01`, [docs/93](../docs/93-SHEETS-CHROME-DESIGN.md)).
///
/// The second of the two selectable chromes, and the pair to `ribbon.js`. It
/// follows exactly the same rule and for exactly the same reason: **it draws a
/// chrome and implements no commands.** Live toolbar controls are borrowed and
/// given back; everything else is a proxy that clicks the owning node and
/// carries `data-oc-proxy`, never a second `data-oc-command` (ADR-026).
///
/// What makes it a different chrome rather than a reskin is the *editorial*
/// rule, which is the thing `docs/93` is actually about. Excel's answer to a
/// hundred commands is a ribbon; Sheets' answer is a short toolbar of about
/// thirty and a menu bar carrying the rest. So this file is mostly a decision
/// about what earns a slot — `TOOLBAR` below — and the menus stay exactly where
/// they already are, drawn as themselves rather than replaced.
///
/// The menu bar is not hidden here, unlike under the ribbon. Sheets has one and
/// ours is already the complete command surface, so the cheapest correct thing
/// is to leave it alone and restyle it.

import { iconIds } from "./ribbon.icons.js?v=2";

/* ── What earns a toolbar slot ────────────────────────────────────────────── */

/// `docs/93`'s selection rule, applied.
///
/// The rule, stated so somebody adding a command next year can apply it without
/// asking: **a slot goes to a verb used many times per session on the cell you
/// are already looking at.** Formatting qualifies. Anything that opens a dialog
/// to ask a question does not — it lives in a menu, one click further away, and
/// the toolbar stays short enough to scan in one pass. That is the whole trade,
/// and it is why Sheets feels lighter than Excel rather than smaller.
///
/// Separators are `null`. Order is left to right, and it is Sheets' order.
const TOOLBAR = [
  "toolbar.undo", "toolbar.redo", "file.print", "toolbar.painter",
  null,
  "toolbar.numfmt", "toolbar.currency", "toolbar.percent",
  "toolbar.dec-dec", "toolbar.inc-dec",
  null,
  "toolbar.font", "toolbar.size-down", "toolbar.size", "toolbar.size-up",
  null,
  "toolbar.bold", "toolbar.italic", "toolbar.strike", "toolbar.fontcolor", "toolbar.fillcolor",
  null,
  "toolbar.border", "toolbar.merge",
  null,
  "toolbar.valign", "toolbar.wrap", "toolbar.rotate",
  null,
  "insert.hyperlink", "insert.note", "insert.chart", "toolbar.filter", "toolbar.freeze",
  null,
  "formulas.insert-function",
];

const PARAMS = new URL(location.href).searchParams;
const home = new WeakMap();
const byCommand = (id) => document.querySelector(`[data-oc-command="${CSS.escape(id)}"]`);

/// What actually has to move when a control is borrowed.
///
/// **The command node is often only part of an assembly.** `#tb-font` is an
/// `<input>` inside a `.menu-wrap` that also holds `#tb-font-caret` and the
/// `#font-menu` popup; the same shape carries the size box and every colour and
/// border picker. Borrowing the input alone moved the box and left its caret and
/// its dropdown behind in a `.toolbar` that this chrome sets to `display: none`
/// — so the font list opened inside a hidden ancestor and nothing appeared.
/// Take the wrapper when there is one, so the assembly stays intact and its
/// popup positions against the control the user just clicked.
const movableFor = (node) => node.closest(".menu-wrap") || node;

function borrow(node, into) {
  if (!home.has(node)) home.set(node, { parent: node.parentElement, next: node.nextElementSibling });
  into.appendChild(node);
}
function giveBack(node) {
  const h = home.get(node);
  if (!h || !h.parent) return;
  // The recorded sibling may have moved too — see `ribbon.js` for the
  // `NotFoundError` this guard exists to prevent.
  const ref = h.next && h.next.parentElement === h.parent ? h.next : null;
  h.parent.insertBefore(node, ref);
}

function labelOf(node, id) {
  const raw = (node.dataset.ocLabel || node.getAttribute("title")
    || node.getAttribute("aria-label") || node.textContent || "").trim();
  return raw.split(/\s*[(—–]|\s+-\s+/)[0].trim() || id.split(".").pop().replace(/-/g, " ");
}

function proxy(node, id) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "gs-btn";
  b.dataset.ocProxy = id;
  b._src = node;   // the owning node, kept so `syncProxies` need not re-query
  const sym = iconIds(id, false).find((c) => document.getElementById(c));
  if (sym) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
    use.setAttribute("href", `#${sym}`);
    svg.appendChild(use);
    b.appendChild(svg);
  } else {
    const s = document.createElement("span");
    s.textContent = labelOf(node, id);
    b.appendChild(s);
    b.classList.add("gs-text");
  }
  const text = labelOf(node, id);
  b.setAttribute("aria-label", text);
  b.title = node.getAttribute("title") || text;
  b.addEventListener("click", (e) => { e.preventDefault(); node.click(); });
  return b;
}

function syncProxies(root) {
  for (const b of root.querySelectorAll("[data-oc-proxy]")) {
    const src = b._src || byCommand(b.dataset.ocProxy);
    if (!src) { b.disabled = true; continue; }
    b.disabled = !!src.disabled || src.classList.contains("oc-cmd-hidden");
    const pressed = src.getAttribute("aria-pressed");
    if (pressed !== null) b.setAttribute("aria-pressed", pressed);
  }
}

let el = null;
let borrowed = [];

let spritePromise = null;
function ensureSprite() {
  if (spritePromise || document.getElementById("oc-ribbon-icons")) return spritePromise;
  spritePromise = fetch(new URL("./ribbon.icons.svg", import.meta.url))
    .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`sprite ${r.status}`))))
    .then((svg) => {
      if (document.getElementById("oc-ribbon-icons")) return;
      const host = document.createElement("div");
      host.style.display = "none";
      // The same sprite `ribbon.js` loads, from the same file in this tree:
      // `<symbol>` definitions only, nothing interpolated. Markup because
      // `<use href="#id">` needs parsed symbols.
      // oc-safe-html: static local sprite, no interpolation.
      host.innerHTML = svg;
      document.body.insertBefore(host.firstElementChild, document.body.firstChild);
    })
    .catch((e) => console.warn("[sheets] icon sprite failed to load", e));
  return spritePromise;
}

export function sheetsRequested() {
  return (PARAMS.get("chrome") || "").toLowerCase() === "sheets";
}

export async function start() {
  if (el) return el;
  await ensureSprite();
  const anchor = document.querySelector(".formula-bar");
  if (!anchor) return null;

  el = document.createElement("div");
  el.className = "oc-sheets";

  const bar = document.createElement("div");
  bar.className = "gs-bar";
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", "Formatting");

  const missing = [];
  for (const id of TOOLBAR) {
    if (id === null) {
      const sep = document.createElement("span");
      sep.className = "gs-sep";
      bar.appendChild(sep);
      continue;
    }
    const node = byCommand(id);
    if (!node) { missing.push(id); continue; }
    if (node.closest(".toolbar")) { node.classList.add("gs-hosted"); borrow(movableFor(node), bar); }
    else bar.appendChild(proxy(node, id));
  }

  // The overflow. Sheets puts what does not fit behind a `⋮`, and so does the
  // editor's own toolbar — but that machinery is wired to `.toolbar`, which is
  // not where these controls are now, so this owns its own.
  const more = document.createElement("button");
  more.type = "button";
  more.className = "gs-more";
  more.title = "More";
  more.setAttribute("aria-label", "More");
  more.setAttribute("aria-haspopup", "true");
  more.setAttribute("aria-expanded", "false");
  more.textContent = "⋮";
  const overflow = document.createElement("div");
  overflow.className = "gs-overflow";
  overflow.hidden = true;
  more.addEventListener("click", (e) => {
    e.stopPropagation();
    overflow.hidden = !overflow.hidden;
    more.setAttribute("aria-expanded", overflow.hidden ? "false" : "true");
  });
  document.addEventListener("click", () => {
    if (!overflow.hidden) { overflow.hidden = true; more.setAttribute("aria-expanded", "false"); }
  });
  // **The overflow popup hangs off `el`, not off the bar.**
  //
  // It was `bar.append(more, overflow)`, which put it *inside* the element being
  // measured — so moving a control into it never reduced `bar.scrollWidth`, the
  // fit loop never satisfied its exit condition, and it ran its full 200
  // iterations forcing a layout on each one. Every resize scheduled another
  // pass, and the renderer stopped responding: a chrome switch took longer than
  // a 45-second debugger timeout. Guarding the loop had hidden the real fault,
  // which is that the measurement could never converge.
  bar.appendChild(more);
  el.appendChild(overflow);

  el.appendChild(bar);
  anchor.parentElement.insertBefore(el, anchor);
  document.documentElement.classList.add("oc-chrome-sheets");

  /// Move what does not fit into the overflow.
  ///
  /// **Arithmetic, not a measure-and-retry loop**, and that is the whole point.
  /// Two earlier versions looped `while (bar.scrollWidth > bar.clientWidth)`,
  /// moving one control per pass. Both hung the renderer: the first because the
  /// popup lived *inside* the element being measured, so the condition could
  /// never become false; the second because `overflow: visible` makes
  /// `scrollWidth` an unreliable answer to "does this fit". A loop whose exit
  /// depends on a measurement it cannot trust is a loop that does not exit, and
  /// a guard only converts a hang into a hundred needless reflows.
  ///
  /// So: measure every child once, add the widths up, and cut the list where
  /// the budget runs out. One layout pass, O(n), no condition to converge on.
  ///
  /// Right to left, because the selection rule puts the daily verbs at the left,
  /// which makes the rightmost slot the most expendable by construction.
  function fit() {
    for (const n of [...overflow.children]) bar.insertBefore(n, more);

    const style = getComputedStyle(bar);
    const pad = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    const gap = parseFloat(style.gap) || 0;
    const budget = bar.clientWidth - pad - more.offsetWidth - gap;
    if (budget <= 0) { more.hidden = true; return; }

    const kids = [...bar.children].filter((n) => n !== more);
    const widths = kids.map((n) => n.offsetWidth + gap);

    let used = widths.reduce((a, b) => a + b, 0);
    let cut = kids.length;
    // Never strip it to nothing: six controls is the floor, below which the row
    // stops being a toolbar and becomes a button that opens a menu.
    while (used > budget && cut > 6) {
      cut -= 1;
      used -= widths[cut];
    }

    for (let i = kids.length - 1; i >= cut; i -= 1) {
      overflow.insertBefore(kids[i], overflow.firstChild);
    }
    // A separator stranded at either end of the row is a rule against nothing.
    for (const sep of [...bar.children]) {
      if (!sep.classList.contains("gs-sep")) continue;
      const prev = sep.previousElementSibling;
      const next = sep.nextElementSibling;
      sep.hidden = !prev || !next || next === more;
    }
    more.hidden = overflow.children.length === 0;
  }

  let queued = false;
  const onResize = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; fit(); });
  };
  window.addEventListener("resize", onResize);
  el._onResize = onResize;

  // One tab stop for the row, arrows within it — the ARIA toolbar pattern, and
  // the same reason the ribbon does it: reachable must not mean thirty-odd Tab
  // presses between the chrome and the grid.
  {
    const stops = () => [...bar.querySelectorAll("button, input")]
      .filter((e) => !e.disabled && e.offsetParent !== null);
    const list = stops();
    for (const e of list) e.tabIndex = -1;
    if (list[0]) list[0].tabIndex = 0;
    bar.addEventListener("keydown", (e) => {
      const dir = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1
        : e.key === "Home" ? "first" : e.key === "End" ? "last" : 0;
      if (!dir) return;
      const l = stops();
      const i = l.indexOf(document.activeElement);
      if (i < 0) return;
      e.preventDefault();
      const next = dir === "first" ? l[0] : dir === "last" ? l[l.length - 1]
        : l[(i + dir + l.length) % l.length];
      for (const el2 of l) el2.tabIndex = -1;
      next.tabIndex = 0;
      next.focus();
    });
  }

  borrowed = [...el.querySelectorAll(".gs-hosted")].map(movableFor);
  fit();
  syncProxies(el);

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
  let syncQueued = false;
  const obs = new MutationObserver(() => {
    if (syncQueued) return;
    syncQueued = true;
    requestAnimationFrame(() => { syncQueued = false; syncProxies(el); });
  });
  for (const b of el.querySelectorAll("[data-oc-proxy]")) {
    if (b._src) obs.observe(b._src, { attributes: true, attributeFilter: ["disabled", "aria-pressed", "class"] });
  }
  el._obs = obs;

  if (missing.length) console.warn(`[sheets] ${missing.length} command(s) absent from this build:`, missing);
  return el;
}

export function stop() {
  if (!el) return;
  el._obs?.disconnect();
  if (el._onResize) window.removeEventListener("resize", el._onResize);
  for (const node of [...borrowed].reverse()) {
    node.classList.remove("gs-hosted");
    // Put the toolbar's roving model back exactly as it was.
    if (node.dataset.rbTabindex !== undefined) {
      if (node.dataset.rbTabindex === "") node.removeAttribute("tabindex");
      else node.setAttribute("tabindex", node.dataset.rbTabindex);
      delete node.dataset.rbTabindex;
    }
    try { giveBack(node); } catch (e) { console.warn("[sheets] could not restore", node.id, e); }
  }
  borrowed = [];
  el.remove();
  el = null;
  document.documentElement.classList.remove("oc-chrome-sheets");
}

export function toggle(on) { on ? start() : stop(); }
