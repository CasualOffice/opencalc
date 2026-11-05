// The status bar and the sheet-tab row, as Excel draws and runs them.
//
// - **Customize Status Bar** (`XL-UX-030`): right-click the bar for Excel's
//   checkable list, persisted as an editor preference. Only the entries whose
//   state exists in OpenCalc are offered (docs/95 rule 9 and the "no dead
//   controls" rule): Excel's Scroll Lock, Fixed Decimal, Overtype Mode, End
//   Mode, Macro Recording, Selection Mode, Page Number, Upload Status, View
//   Shortcuts and the Flash Fill / Signatures / Permissions rows describe
//   features this editor does not have, and a tick that changes nothing is a
//   fake control.
// - **Messages end with the selection** (`XL-UX-032`): the bar shows the cell
//   mode and brief prompts. A message a command leaves there is taken down by
//   the next selection change; the few lines that are *states* rather than
//   messages (the collaboration connection) are written with `holdStatus()`.
// - **The tab-row splitter** (`XL-UX-033`, `XL-UX-052`): the horizontal
//   scrollbar shares the sheet-tab row, and the box between them trades one's
//   room for the other's.

import {
  byId,
  draw,
  fmtNum,
  invalidateTrackSize,
  modeEl,
  ocOverlayHost,
  state,
  status,
} from "./editor.core.js";
import { selectionSummary, updateStats } from "./editor.selection.js";

// --- Preferences --------------------------------------------------------------

/// Excel's rows that OpenCalc can honour, in Excel's menu order, with Excel's
/// defaults (support.microsoft.com, "Excel status bar options"). The order of
/// Zoom Slider and Zoom follows the menu in Excel for Microsoft 365, which puts
/// the slider first as the bar does [unverified: the support page lists Zoom
/// first].
export const STATUS_ITEMS = [
  { key: "cellMode", label: "Cell Mode", on: true },
  { key: "capsLock", label: "Caps Lock", on: false },
  { key: "numLock", label: "Num Lock", on: false },
  { key: "average", label: "Average", on: true, sep: true },
  { key: "count", label: "Count", on: true },
  { key: "numericalCount", label: "Numerical Count", on: false },
  { key: "minimum", label: "Minimum", on: false },
  { key: "maximum", label: "Maximum", on: false },
  { key: "sum", label: "Sum", on: true },
  { key: "zoomSlider", label: "Zoom Slider", on: true, sep: true },
  { key: "zoom", label: "Zoom", on: true },
];

const PREF_KEY = "oc-status-bar";
let prefs = null;

function loadPrefs() {
  const out = {};
  for (const it of STATUS_ITEMS) out[it.key] = it.on;
  try {
    const saved = JSON.parse(localStorage.getItem(PREF_KEY) || "{}");
    for (const it of STATUS_ITEMS) if (typeof saved[it.key] === "boolean") out[it.key] = saved[it.key];
  } catch {}
  return out;
}

/// Whether the bar shows `key`. Read on every `updateStats()`, so it is a
/// lookup in a held object rather than a `localStorage` parse.
export function statusPref(key) {
  if (!prefs) prefs = loadPrefs();
  return !!prefs[key];
}

function setPref(key, on) {
  if (!prefs) prefs = loadPrefs();
  prefs[key] = on;
  // Only what differs from Excel's defaults is stored, so a default that is
  // ever corrected reaches people who never touched it.
  const diff = {};
  for (const it of STATUS_ITEMS) if (prefs[it.key] !== it.on) diff[it.key] = prefs[it.key];
  try { localStorage.setItem(PREF_KEY, JSON.stringify(diff)); } catch {}
  applyPrefs();
}

function applyPrefs() {
  const bar = byId("status-bar");
  if (!bar) return;
  bar.classList.toggle("sb-off-cell-mode", !statusPref("cellMode"));
  bar.classList.toggle("sb-off-zoom-slider", !statusPref("zoomSlider"));
  bar.classList.toggle("sb-off-zoom", !statusPref("zoom"));
  renderLocks();
  updateStats();
}

// --- Lock keys ------------------------------------------------------------------

/// What the browser last reported. A page cannot ask for the lock state; every
/// key and pointer event carries it, so it is known from the first one.
const locks = { caps: false, num: false };

function renderLocks() {
  const caps = byId("sb-caps-lock");
  const num = byId("sb-num-lock");
  if (caps) caps.hidden = !(statusPref("capsLock") && locks.caps);
  if (num) num.hidden = !(statusPref("numLock") && locks.num);
}

function noteLocks(e) {
  if (typeof e.getModifierState !== "function") return;
  const caps = e.getModifierState("CapsLock");
  const num = e.getModifierState("NumLock");
  if (caps === locks.caps && num === locks.num) return;
  locks.caps = caps;
  locks.num = num;
  renderLocks();
}

// --- Messages that end with the selection ------------------------------------

/// Text nodes written by `holdStatus()`: states, not messages.
const held = new WeakSet();
/// The text of the bar and the selection it was written under. Stamped by a
/// `MutationObserver` rather than from `draw()`: a command that writes its
/// message and does not redraw would otherwise be stamped at the *next*
/// selection — the one that should have taken it down — and outlive it. The
/// observer runs at the end of the task that wrote, so a command that writes
/// and then moves the selection (a paste, a sort) is stamped where it left the
/// selection, which is the selection its message is about.
let seenText = null;
let seenAt = "";

/// Write a line that describes a standing state — the collaboration
/// connection — and so must not be taken down by a selection change.
export function holdStatus(text) {
  if (!status) return;
  status.textContent = text;
  if (status.firstChild) held.add(status.firstChild);
}

function selectionKey() {
  const s = state.sel, a = state.anchor || s;
  return `${state.sheet}:${a.row},${a.col}:${s.row},${s.col}:${state.ranges.length}:${state.selKind}`;
}

function stamp() {
  seenText = status ? status.textContent : "";
  seenAt = selectionKey();
}

/// Called every frame from `draw()`. Cheap: a string compare in the steady
/// state.
///
/// When the selection moves on from the one a message was written under, the
/// message goes (`XL-UX-032`). Errors (`.err`) have their own rule, scoped to
/// the active cell (`UX-SHELL-03`); a file job's progress line stays while the
/// job runs; and a held state stays until something replaces it. A `.warn`
/// stays too: it is the import report naming what an open did not keep
/// (`reportImportIssues`), or the time budget's "stopped before it finished"
/// beside its Keep waiting button — the bar is the only place either is said,
/// and the "no silent data loss" contract does not end with an arrow key.
export function dropStaleStatusMessage() {
  if (!status) return;
  const text = status.textContent;
  // Written since the observer last ran (it is about to): leave it to stamp.
  if (seenText === null || text !== seenText) return;
  if (!text || selectionKey() === seenAt) return;
  if (status.querySelector(".err, .warn") || byId("file-progress")) return;
  if (status.firstChild && held.has(status.firstChild)) return;
  // End mode is armed until the next arrow, which `setEndMode` clears itself.
  if (state.endMode) return;
  status.textContent = "";
  seenText = "";
}

// --- Customize Status Bar --------------------------------------------------------

function itemValue(key) {
  const sum = selectionSummary();
  const num = (v) => (sum && sum.numeric > 0 ? fmtNum(v) : "");
  switch (key) {
    case "cellMode": return modeEl ? modeEl.textContent : "";
    case "capsLock": return locks.caps ? "On" : "Off";
    case "numLock": return locks.num ? "On" : "Off";
    case "average": return sum ? num(sum.sum / sum.numeric) : "";
    case "count": return sum ? String(sum.count) : "";
    case "numericalCount": return sum ? String(sum.numeric) : "";
    case "minimum": return sum ? num(sum.min) : "";
    case "maximum": return sum ? num(sum.max) : "";
    case "sum": return sum ? num(sum.sum) : "";
    case "zoom": return `${Math.round((state.zoom || 1) * 100)}%`;
    default: return "";
  }
}

/// Where the keyboard was when the menu opened — the grid, nearly always.
/// The menu takes focus for its arrow keys; closing it must hand focus back,
/// or it falls to <body> and the next entry typed reaches nothing, where
/// Excel's Escape returns straight to the worksheet.
let returnFocus = null;

export function closeStatusMenu({ restoreFocus = true } = {}) {
  const m = byId("status-customize");
  if (!m) return;
  const back = returnFocus;
  returnFocus = null;
  m.remove();
  document.removeEventListener("mousedown", onOutside, true);
  document.removeEventListener("keydown", onMenuKey, true);
  if (restoreFocus && back && back.isConnected) back.focus({ preventScroll: true });
}

function onOutside(e) {
  const m = byId("status-customize");
  // A click away lands focus where it was aimed; only an empty landing (the
  // bar itself, which takes no focus) falls back to where the keyboard was.
  if (m && !e.composedPath().includes(m)) {
    const back = returnFocus;
    closeStatusMenu({ restoreFocus: false });
    setTimeout(() => {
      const a = document.activeElement;
      if ((!a || a === document.body) && back && back.isConnected) back.focus({ preventScroll: true });
    }, 0);
  }
}

function onMenuKey(e) {
  const m = byId("status-customize");
  if (!m) return;
  const items = [...m.querySelectorAll("[role=menuitemcheckbox]")];
  const at = items.indexOf(m.getRootNode().activeElement);
  if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeStatusMenu(); return; }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault(); e.stopPropagation();
    const step = e.key === "ArrowDown" ? 1 : -1;
    items[(at + step + items.length) % items.length]?.focus();
  }
}

export function openStatusMenu(x, y) {
  closeStatusMenu({ restoreFocus: false });
  const active = document.activeElement;
  returnFocus = active && active !== document.body ? active : byId("grid");
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu sb-menu";
  menu.id = "status-customize";
  menu.setAttribute("role", "menu");
  menu.setAttribute("aria-label", "Customize Status Bar");
  const title = document.createElement("div");
  title.className = "sb-menu-title";
  title.textContent = "Customize Status Bar";
  menu.appendChild(title);
  for (const it of STATUS_ITEMS) {
    if (it.sep) menu.appendChild(document.createElement("div")).className = "menu-sep";
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "menuitemcheckbox");
    b.dataset.key = it.key;
    const label = document.createElement("span");
    label.className = "sb-item-label";
    label.textContent = it.label;
    const value = document.createElement("span");
    value.className = "sb-item-value";
    b.append(label, value);
    const paint = () => {
      b.setAttribute("aria-checked", String(statusPref(it.key)));
      value.textContent = itemValue(it.key);
    };
    paint();
    // Excel's menu stays open on a tick, so several can be changed at once.
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      setPref(it.key, !statusPref(it.key));
      paint();
    });
    menu.appendChild(b);
  }
  menu.style.left = "0px";
  menu.style.top = "0px";
  menu.style.visibility = "hidden";
  ocOverlayHost.appendChild(menu);
  const h = menu.offsetHeight, w = menu.offsetWidth;
  // Above the pointer when it does not fit below — which, from a bar at the
  // bottom of the window, is nearly always.
  menu.style.top = `${y + h > window.innerHeight ? Math.max(4, y - h) : y}px`;
  menu.style.left = `${x + w > window.innerWidth ? Math.max(4, x - w) : x}px`;
  menu.style.visibility = "visible";
  // Focused for the keyboard, without a ring for a mouse-opened menu.
  menu.querySelector("[role=menuitemcheckbox]")?.focus({ focusVisible: false });
  document.addEventListener("mousedown", onOutside, true);
  document.addEventListener("keydown", onMenuKey, true);
}

// --- The tab-row splitter ------------------------------------------------------

function wireSplitter() {
  const split = byId("sheet-bar-split");
  const tabs = byId("sheet-tabs");
  const bar = byId("sheet-bar");
  const track = byId("hscroll");
  if (!split || !tabs || !bar) return;
  const MIN_TABS = 60, MIN_TRACK = 48;
  const setWidth = (px) => {
    const room = track ? tabs.getBoundingClientRect().width + track.getBoundingClientRect().width : px;
    const w = Math.max(MIN_TABS, Math.min(px, room - MIN_TRACK));
    bar.style.setProperty("--oc-tab-split", `${Math.round(w)}px`);
  };
  let drag = null;
  split.addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    drag = { x: e.clientX, w: tabs.getBoundingClientRect().width };
    split.classList.add("drag");
  });
  window.addEventListener("mousemove", (e) => {
    if (!drag) return;
    setWidth(drag.w + e.clientX - drag.x);
  });
  window.addEventListener("mouseup", () => {
    if (!drag) return;
    drag = null;
    split.classList.remove("drag");
  });
  // Excel's double-click puts the split back where it started.
  split.addEventListener("dblclick", () => bar.style.removeProperty("--oc-tab-split"));
  split.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    setWidth(tabs.getBoundingClientRect().width + (e.key === "ArrowRight" ? 24 : -24));
  });
  // The thumb is laid out against the track's width, which `updateScrollbars`
  // caches; any change to the track — the splitter, a window resize, a host
  // hiding the tabs — has to drop that cache or the thumb runs off its track.
  if (track && typeof ResizeObserver === "function") {
    new ResizeObserver(() => { invalidateTrackSize(); draw(); }).observe(track);
  }
}

// --- Wiring ----------------------------------------------------------------------

let wired = false;

/// Called once at boot, after the status bar's controls exist.
export function wireStatusBar() {
  if (wired) return;
  wired = true;
  prefs = loadPrefs();
  applyPrefs();
  const bar = byId("status-bar");
  bar?.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    openStatusMenu(e.clientX, e.clientY);
  });
  for (const type of ["keydown", "keyup", "mousedown"]) {
    document.addEventListener(type, noteLocks, true);
  }
  wireSplitter();
  if (status) {
    stamp();
    new MutationObserver(stamp).observe(status, { childList: true, characterData: true, subtree: true });
  }
}
