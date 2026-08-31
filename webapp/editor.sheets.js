// Sheet-level and document-level commands: tabs, saving, printing,
// protection and personal views.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import { syncVolatileClock } from "./editor.selection.js";
import {
  A1,
  HH,
  HW,
  ZOOM_MAX,
  ZOOM_MIN,
  afterFilterChange,
  allRanges,
  applyCommandRules,
  byId,
  clearHeightMemo,
  clearKeepWaiting,
  colAtX,
  commit,
  confirmModal,
  decodeTextBytes,
  download,
  draw,
  endInline,
  errText,
  formatSel,
  friendlyOpenError,
  imageCache,
  invalidateGrowth,
  off,
  offerKeepWaiting,
  on,
  openBudgetMs,
  recalcBudgetMs,
  renderPresence,
  renderTabs,
  reportImportIssues,
  resize,
  rowAtY,
  select,
  sheetNameAt,
  sheetViews,
  state,
  status,
  statusError,
  stopMarch,
  syncClock,
  // A live binding: `editor.core.js` assigns it at mount, and this module reads
  // it only after boot has called `wireZoom()`.
  tabsEl,
  tryEdit,
  wasStopped,
  wasm,
} from "./editor.core.js";

export function setZoom(z) {
  const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z * 100) / 100));
  if (next === state.zoom) {
    // Still redraw the widget. A slider dragged past the end, or a `−` at 25%,
    // clamps to the level already in force — and the control that moved is
    // showing the value it was dragged to, not the one that took effect. The
    // early return is about the expensive relayout below, not about the chrome.
    renderZoom();
    return;
  }
  state.zoom = next;
  // Text is measured in zoom-logical units, so every grown row's height changes.
  clearHeightMemo();
  invalidateGrowth();
  resize();
  status.textContent = `zoom ${Math.round(next * 100)}%`;
  renderZoom();
}

/// The zoom readout in the status bar, redrawn from `state.zoom`.
///
/// Called from `setZoom` rather than from each control, because `setZoom` is
/// the only writer of `state.zoom` in the editor — the View ▸ Zoom submenu,
/// `Ctrl+Alt+0`, `Ctrl`+wheel, a trackpad pinch and these three controls all go
/// through it. A widget that updated itself would be right for the two buttons
/// beside it and stale for the four routes that are not.
export function renderZoom() {
  const level = byId("zoom-level");
  const slider = byId("zoom-slider");
  const out = byId("zoom-out");
  const zin = byId("zoom-in");
  // A host that hid the status bar, or a call before the mount is bound.
  if (!level) return;
  const pct = Math.round((state.zoom || 1) * 100);
  level.textContent = `${pct}%`;
  if (slider) slider.value = String(pct);
  // Disabled at the ends rather than silently doing nothing: a button that
  // still looks live and no longer moves anything reads as a broken editor.
  if (out) out.disabled = state.zoom <= ZOOM_MIN;
  if (zin) zin.disabled = state.zoom >= ZOOM_MAX;
}

/// Bind the status-bar zoom controls. Called once, from boot.
export function wireZoom() {
  const level = byId("zoom-level");
  const slider = byId("zoom-slider");
  if (!level) return;
  // A tenth at a time, matching the `Ctrl`+wheel step, so the two routes to the
  // same thing do not land on different ladders.
  byId("zoom-out")?.addEventListener("click", () => setZoom(state.zoom / 1.1));
  byId("zoom-in")?.addEventListener("click", () => setZoom(state.zoom * 1.1));
  level.addEventListener("click", () => setZoom(1));
  // `input`, not `change`: the grid follows the thumb while it is dragged, which
  // is what makes a zoom slider usable at all.
  slider?.addEventListener("input", () => setZoom(Number(slider.value) / 100));
  renderZoom();
  // The bottom bar's other controls — the sheet-tab navigation. Mounted here
  // because `wireZoom()` is the only function of this module the boot sequence
  // calls, and it runs after `renderTabs()` has built the strip. See
  // `wireSheetNav()`.
  wireSheetNav();
}

// ---------------------------------------------------------------------------
// The pinned sheet-navigation rail (`UX-CHR-07`).
//
// **The defect, from a running editor rather than from reading it.**
// `renderTabs()` appends the add-sheet `+` and the all-sheets `☰` *into*
// `#sheet-tabs`, which is the `overflow-x: auto` element. At 1280px the strip
// is 796px wide and twelve default-named sheets are 897px of content, so both
// controls are pushed past its right edge — and the strip has **no scroll
// arrows**, so no control in the chrome brings them back. Measured: the
// controls go out of reach at 8 sheets at 1024px, 12 at 1280px, 14 at 1440px,
// 15 at 1512px and 20 at 1920px. A user with twelve sheets on a laptop could
// not add a thirteenth.
//
// All four desktop competitors pin these outside the scroller, three of them on
// the left, which is the only placement tabs cannot push away
// (docs/88 §5).
//
// **Why the rail adopts the buttons rather than building its own.** The two
// buttons — and the all-sheets menu behind `☰` — are built by `renderTabs()` in
// `editor.core.js`, which rebuilds them on every render. This moves those very
// nodes into the rail, so there is exactly one add-sheet handler and one
// all-sheets menu in the editor; a second pair here would be a second thing to
// keep in step, and the first one to diverge. The move is driven by a
// `MutationObserver` because `renderTabs()` has no hook this module can take.
//
// The layout properties below are set on the elements rather than in
// `editor.css`, and everything *painted* — size, radius, hover, colour — comes
// from the existing `.sheet-add` class, so the rail stays inside the token
// system and no rule was added to the stylesheet for it.

/// The rail, and the two buttons it adopts from each render.
let navRail = null;
let navPrev = null;
let navNext = null;
let navAdd = null;
let navAll = null;

/// One chevron, in the toolbar's icon idiom.
function chevron(points) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("class", "icon-sm");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
  path.setAttribute("points", points);
  svg.appendChild(path);
  return svg;
}

function navButton(cls, label, points) {
  const b = document.createElement("button");
  // `.sheet-add` is the 26px square button of this strip; the second class is
  // what tells the three of them apart.
  b.className = "sheet-add " + cls;
  b.type = "button";
  b.title = label;
  b.setAttribute("aria-label", label);
  b.appendChild(chevron(points));
  return b;
}

/// Scroll the strip by one tab, in `dir`.
///
/// By a tab rather than by a fixed number of pixels: a tab is the unit the user
/// is looking for, and a pixel step leaves one sliced in half at the edge —
/// which is what Excel's and OnlyOffice's single arrows avoid.
function stepTabs(dir) {
  const tabs = [...tabsEl.querySelectorAll(".sheet-tab")];
  const left = tabsEl.scrollLeft;
  const right = left + tabsEl.clientWidth;
  if (dir > 0) {
    const next = tabs.find((t) => t.offsetLeft + t.offsetWidth > right + 1);
    tabsEl.scrollLeft = next
      ? next.offsetLeft + next.offsetWidth - tabsEl.clientWidth
      : tabsEl.scrollWidth;
  } else {
    const prev = tabs.reverse().find((t) => t.offsetLeft < left - 1);
    tabsEl.scrollLeft = prev ? prev.offsetLeft : 0;
  }
  syncSheetNav();
}

/// Adopt whatever the last render appended, and re-state the arrows.
///
/// Runs after every rebuild of the strip and after every scroll of it, so it
/// has to be cheap and idempotent — it is both: two `querySelector`s and four
/// property writes.
function syncSheetNav() {
  if (!navRail || !tabsEl) return;

  // A host can hide the tab strip (`.oc-hide-tabs`); the rail belongs to the
  // strip, so it goes with it. Read from the layout rather than from the class,
  // because the class is on an ancestor this module does not own — and a
  // `display: none` strip reports no client rects, which is also what makes the
  // `ResizeObserver` below the thing that notices a host toggling it.
  const shown = tabsEl.getClientRects().length > 0;
  navRail.style.display = shown ? "flex" : "none";

  // `renderTabs()` builds a fresh pair on every render, so the old pair has to
  // go or the rail grows one `+` per rebuild.
  const add = tabsEl.querySelector(".sheet-add:not(.sheet-all)");
  const all = tabsEl.querySelector(".sheet-all");
  if (add) {
    navAdd?.remove();
    navAdd = add;
    // The add button is the only one of the three with no name of its own.
    add.classList.add("sheet-new");
    navRail.appendChild(add);
  }
  if (all) {
    navAll?.remove();
    navAll = all;
    navRail.appendChild(all);
  }

  // Arrows only while there is something to scroll. Excel greys them out
  // always; here the bottom bar is shared with the cell mode, the selection
  // summary and the zoom widget, and 52px of permanently dead control is 52px
  // the tabs do not get on a narrow window.
  const over = tabsEl.scrollWidth - tabsEl.clientWidth > 1;
  const atStart = tabsEl.scrollLeft <= 0;
  const atEnd = tabsEl.scrollLeft >= tabsEl.scrollWidth - tabsEl.clientWidth - 1;
  for (const [b, off] of [[navPrev, atStart], [navNext, atEnd]]) {
    // "" rather than a value: `.sheet-add`'s own `inline-flex` is what should
    // apply when the button is shown.
    b.style.display = over ? "" : "none";
    b.disabled = !over || off;
    // A disabled button still takes `:hover` from the stylesheet, which reads
    // as live. This is the one piece of state `.sheet-add` has no rule for.
    b.style.opacity = b.disabled ? "0.35" : "";
    b.style.pointerEvents = b.disabled ? "none" : "";
  }
}

/// Build the rail and start watching the strip. Called once, from `wireZoom()`.
function wireSheetNav() {
  if (navRail || !tabsEl || !tabsEl.parentNode) return;

  navRail = document.createElement("div");
  navRail.className = "sheet-nav";
  navRail.id = "sheet-nav";
  // Layout only — see the block comment above on why this is not in the
  // stylesheet.
  navRail.style.display = "flex";
  navRail.style.alignItems = "center";
  navRail.style.gap = "2px";
  navRail.style.flex = "0 0 auto";

  navPrev = navButton("sheet-scroll sheet-scroll-prev", "Scroll tabs left", "15 18 9 12 15 6");
  navNext = navButton("sheet-scroll sheet-scroll-next", "Scroll tabs right", "9 18 15 12 9 6");
  navPrev.addEventListener("click", () => stepTabs(-1));
  navNext.addEventListener("click", () => stepTabs(1));
  navRail.append(navPrev, navNext);

  // Pinned to the left of the strip, which is where three of the four desktop
  // competitors put it and the only side tabs cannot push it off.
  tabsEl.parentNode.insertBefore(navRail, tabsEl);

  // **Make the strip the tabs' `offsetParent`** — a separate defect, found
  // while measuring this one.
  //
  // `renderTabs()` keeps the active tab in view with
  // `tabsEl.scrollLeft = activeTab.offsetLeft`, which is only true if
  // `offsetLeft` is measured inside the strip. `.sheet-tabs` is statically
  // positioned, so it is not an `offsetParent` and the tabs measure from
  // `<body>` instead: every one of those numbers is inflated by the strip's own
  // distance from the left of the page. Today that is ~14px and the
  // over-scroll is invisible; the rail moves the strip ~120px right, which
  // would have turned an invisible bug into "jump to Sheet1 and it is not on
  // screen". One line, and the arithmetic in `renderTabs()` becomes true
  // instead of nearly true.
  tabsEl.style.position = "relative";

  // Three things change what the rail should show: a rebuild of the strip
  // (adds, deletes, renames, undo), a scroll of it, and a resize of it — the
  // last of which is also how a host hiding the strip is noticed.
  new MutationObserver(() => syncSheetNav()).observe(tabsEl, { childList: true });
  tabsEl.addEventListener("scroll", () => syncSheetNav(), { passive: true });
  new ResizeObserver(() => syncSheetNav()).observe(tabsEl);

  // `renderTabs()` has already run by the time boot reaches here, so the first
  // adoption is this call rather than the observer's.
  syncSheetNav();
}

export function commitFreezeDrag(axis, px, py) {
  const F = state.freeze;
  let fr = F.fr, fc = F.fc;
  if (axis === "col") {
    fc = px <= HW + 2 ? 0 : Math.max(0, colAtX(px));
  } else {
    fr = py <= HH + 2 ? 0 : Math.max(0, rowAtY(py));
  }
  try { wasm.session_set_freeze(state.sheet, fr, fc); } catch (e) { statusError(errText(e)); }
  status.textContent = (fc || fr) ? "freeze updated" : "unfrozen";
}

export function setEndMode(on) {
  if (state.endMode === on) return;
  state.endMode = on;
  if (status) status.textContent = on ? "End mode — press an arrow key" : "";
}

export function readOutline(first, count, columns) {
  const f = columns ? state.freeze.fc : state.freeze.fr;
  const from = f > 0 ? 0 : first;
  const span = (first - from) + count + 2;
  const out = {};
  try {
    const { lines } = JSON.parse(wasm.session_outline(state.sheet, from, span, columns));
    lines.forEach((l, i) => { out[from + i] = l; });
  } catch {}
  return out;
}

export function setCellProtection(which) {
  let now = false;
  try {
    now = !!JSON.parse(
      wasm.session_cell_protection(state.sheet, state.sel.row, state.sel.col))[which];
  } catch {}
  formatSel((s) => wasm.session_set_cell_protection(
    state.sheet, s.r0, s.c0, s.r1, s.c1, which, !now));
  status.textContent = which === "locked"
    ? (now ? "unlocked — takes effect while the sheet is protected"
           : "locked — takes effect while the sheet is protected")
    : (now ? "formula shown" : "formula hidden while the sheet is protected");
}

export function sheetProtectedNow() {
  try { return !!JSON.parse(wasm.session_sheet_protected())[state.sheet]; }
  catch { return false; }
}

export function toggleSheetProtected() {
  const now = sheetProtectedNow();
  tryEdit(() => wasm.session_set_sheet_protected(state.sheet, !now));
  status.textContent = now ? "sheet unprotected" : "sheet protected — locked cells refuse edits";
}

export function viewOptions() {
  try { return JSON.parse(wasm.session_view_options(state.sheet)); }
  catch { return { formulas: false, zeros: true }; }
}

export function setViewOption(which) {
  const now = !!viewOptions()[which];
  tryEdit(() => wasm.session_set_view_option(state.sheet, which, !now));
}

export function setFreeze(kind) {
  if (kind === "gridlines") {
    try { wasm.session_set_gridlines_hidden(state.sheet, !wasm.session_gridlines_hidden(state.sheet)); }
    catch (e) { statusError(errText(e)); }
    draw();
    return;
  }
  let rows = 0, cols = 0;
  if (kind === "sel") { rows = state.sel.row; cols = state.sel.col; }
  else if (kind === "row") rows = 1;
  else if (kind === "col") cols = 1;
  try {
    wasm.session_set_freeze(state.sheet, rows, cols);
    state.scrollX = state.scrollY = 0;
  } catch (e) { statusError(errText(e)); }
  draw();
}

export function applyPersonalFilter(col, values) {
  if (!wasm) return;
  try {
    const rows = wasm.session_rows_hidden_by_values(state.sheet, col, JSON.stringify(values));
    wasm.session_set_personal_filter(state.sheet, rows);
    const n = JSON.parse(rows).length;
    status.textContent = n
      ? `${n} row${n === 1 ? "" : "s"} hidden — for you only`
      : "your view shows every row";
  } catch (why) {
    status.textContent = "could not apply your view";
    console.error("[opencalc] personal filter", why);
  }
}

export function clearMyView() {
  if (!wasm) return;
  try {
    wasm.session_clear_all_personal_views();
    status.textContent = "your view cleared — showing every row";
    afterFilterChange();
  } catch (why) {
    console.error("[opencalc] clear view", why);
  }
}

export function calcMode() {
  try { return wasm.session_calculation_mode(); } catch { return "auto"; }
}

export function setCalculationMode(mode) {
  try { wasm.session_set_calculation_mode(mode); } catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  status.textContent = mode === "manual"
    ? "manual calculation — press F9 to calculate"
    : "automatic calculation";
}

export function recalculateNow(budgetMs = undefined) {
  // An explicit recalculation is exactly when Excel rerolls `RAND` and moves
  // `NOW` on, so the clock is refreshed first (`CALC-VOL-01`).
  syncVolatileClock();
  let outcome = "full";
  clearKeepWaiting();
  tryEdit(() => {
    syncClock(true);
    wasm.session_set_time_budget_ms(budgetMs === undefined ? recalcBudgetMs : budgetMs);
    try {
      outcome = wasm.session_recalculate();
    } finally {
      wasm.session_clear_time_budget();
    }
  });
  if (outcome === "cancelled" || outcome === "over-budget") {
    // Not "recalculated": a stopped pass keeps what it computed, so the sheet
    // is a mixture of fresh and stale values. Saying it finished would be the
    // one thing a host must not do with a cancelled recalculation — the
    // workbook still reports itself as needing one, and so does this.
    statusError("calculation stopped — some values are still out of date");
    offerKeepWaiting("calculating", () => recalculateNow(-1));
    return;
  }
  status.textContent = "recalculated";
}

export function readOnly() {
  try { return wasm.session_read_only(); } catch { return false; }
}

export function setReadOnly(on) {
  try { wasm.session_set_read_only(!!on); } catch (e) { statusError(errText(e)); return; }
  // Editing commands come off the menus and the toolbar, not just the
  // keystroke path: a viewer offered Insert ▸ Chart and told "no" after
  // clicking has been misled, twice.
  applyCommandRules();
  // Editing chrome is pointless in a viewer, but hiding it is the *host's*
  // call — the engine's refusal is what makes the mode real, so here we only
  // repaint and let the mode word say so.
  invalidateGrowth();
  draw();
}

export function needsRecalc() {
  try { return wasm.session_needs_recalculation(); } catch { return false; }
}

export function printSheet() {
  let html = "";
  try { html = wasm.session_print_html(state.sheet); } catch (e) { statusError(errText(e)); return; }
  if (!html) { status.textContent = "nothing to print"; return; }
  const w = window.open("", "_blank");
  if (!w) { statusError("the browser blocked the print window"); return; }
  // oc-safe-html: `session_print_html` builds the page in Rust and escapes
  // every workbook string through `push_html_escaped`. The markup is the
  // engine's, not the file's.
  // oc-safe-html: see the note above.
  w.document.write(html);
  w.document.close();
  // Printing before the document has laid out gives a blank first page.
  w.addEventListener("load", () => { w.focus(); w.print(); });
}

export function commentAuthor() {
  try { return localStorage.getItem("oc-comment-author") || ""; } catch { return ""; }
}

export function setCommentAuthor(name) {
  try { localStorage.setItem("oc-comment-author", name); } catch {}
}

export function commentStamp() {
  return new Date().toISOString().replace("Z", "").slice(0, 22);
}

export function readThread(row, col) {
  try { return JSON.parse(wasm.session_comment_thread(state.sheet, row, col)); }
  catch { return null; }
}

export function clearAll() {
  try { for (const s of allRanges()) wasm.session_clear_range(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { statusError(errText(e)); }
  draw();
}

// --- Unsaved work -----------------------------------------------------------
//
// The document lives in wasm memory and nowhere else: there is no autosave, no
// draft, and until now no `beforeunload`. Closing the tab, hitting Ctrl+R, or
// pressing Back discarded an hour of work without a word.
//
// The dirty check asks the engine rather than keeping a tally here. A tally in
// the editor is a list of every mutation it can perform, and this repository has
// already learned twice what a rule that enumerates its subjects costs: it is
// one omission away from being wrong, and the omission is always the write path
// somebody added last.
let savedAtEdits = 0;

function editsApplied() {
  try {
    return wasm ? wasm.session_edits_applied() : 0;
  } catch {
    // NaN compares unequal to everything, including itself, so a failed read
    // reports *dirty*. That is the safe direction: a needless warning costs a
    // click, and the other mistake costs the document.
    return Number.NaN;
  }
}

/// The document is now on disk, or freshly loaded: this is the state to compare
/// against.
export function markSaved() {
  savedAtEdits = editsApplied();
}

/// Whether anything has changed since the last save or load.
export function isDirty() {
  return editsApplied() !== savedAtEdits;
}

/// The counter at the last save or load — the *baseline*, not the verdict.
///
/// `isDirty()` collapses this to a boolean, and a boolean is not enough for a
/// draft. A recovery bar has to state how far ahead of the last save the draft
/// is (`docs/83` §4.3), and it has to leave a draft level with the last save
/// unoffered — neither of which "true" can express. So the number is exported
/// for `editor.drafts.js` and read nowhere else.
export function savedAtEditsForDraft() {
  return savedAtEdits;
}

// --- What this build can write ----------------------------------------------
//
// `IO-07` and `IO-08` gave the engine ODS and macro-enabled `.xlsm`, and for a
// while the editor could reach neither: `saveAs` knew four formats by name and
// the Download submenu listed five entries by hand. That is the same shape the
// shell's own format list had before `ODS-01` — a second list, drifting, greying
// out a format the engine reads perfectly — so the answer is the same one:
// **ask the engine**.

/// Extensions this build can write, as the engine reports them.
///
/// `writable_extensions()` is deliberately narrower than what the engine
/// *reads*: `.tab` names the TAB delimiter, whose own extension is `tsv`, so it
/// is a name this engine opens and does not write. Offering it would be a menu
/// entry whose save then refuses.
///
/// The floor on failure is the document's own format rather than nothing: a
/// build that cannot answer this question can still write the file it opened,
/// and a Download submenu with no entries is worse than one with too few.
export function writableFormats() {
  try {
    return JSON.parse(wasm.writable_extensions())
      .map((x) => String(x).replace(/^[."']+|["']+$/g, ""))
      .filter(Boolean);
  } catch (err) {
    console.error("[opencalc] writable_extensions", err);
    return ["xlsx"];
  }
}

/// What a human calls a format, for the Download submenu.
///
/// The four that existed keep their exact wording, because the command id is
/// slugged from the label (`commandId()`), and a reworded entry is a renamed
/// command that every host rule naming it stops matching.
const FORMAT_LABELS = {
  xlsx: "Excel (.xlsx)",
  xlsm: "Excel macro-enabled (.xlsm)",
  ods: "OpenDocument (.ods)",
  csv: "CSV (.csv)",
  tsv: "Tab-separated (.tsv)",
  psv: "Pipe-separated (.psv)",
};

/// The Download submenu, derived from the engine rather than written out.
///
/// "Same format as opened" leads and is named for what it does rather than for
/// a format: it is the only one of these that gives back the kind of file that
/// was opened. The others are conversions, and a conversion chosen by accident
/// is how a `.csv` comes back as a package under its own name.
///
/// A format the engine learns appears here the day it does.
export function downloadItems() {
  return [
    ["Same format as opened", () => saveAs("native", "download")],
    ...writableFormats().map((ext) => [
      FORMAT_LABELS[ext] || `${ext.toUpperCase()} (.${ext})`,
      () => saveAs(ext, "download"),
    ]),
  ];
}

/// Put the cost of a format to the user, **before** any bytes exist.
///
/// `session_save_loss_for` takes the format the person picked, not the one they
/// opened — and that distinction is the whole of `IO-08`'s user-facing half. A
/// two-sheet workbook loses its other sheets to `.csv` and nothing to `.ods`; a
/// macro workbook loses its macros to both. `session_save_loss()` could not tell
/// those apart because it only ever answered about the session's own format, so
/// `File ▸ Download ▸ Excel (.xlsx)` on a macro-enabled workbook said nothing at
/// all while the VBA project was dropped.
///
/// Returns whether to go ahead. An empty report is not a question.
async function confirmLoss(ext, verb) {
  let loss = "";
  try { loss = wasm.session_save_loss_for(ext); } catch {}
  if (!loss) return true;
  return await confirmModal(
    `.${ext} cannot carry all of this`,
    `${loss}. The file will hold everything a ${ext.toUpperCase()} file can, and nothing else.`,
    `${verb} .${ext}`,
  );
}

/// Write the document as `ext`, whatever it was opened from.
///
/// **Always a download, never a save.** This is the conversion path — the user
/// picked a format that is not necessarily the document's — and `docs/83` §2 is
/// clear that a copy in another format is not where the document lives now. So
/// it never acquires a save target and never moves one.
export async function doSaveFormat(ext) {
  if (!(await confirmLoss(ext, "Download"))) return false;
  let written;
  try {
    written = await download(
      wasm.session_save_as(ext),
      `opencalc.${ext}`,
      wasm.format_content_type(ext),
    );
  } catch (e) {
    statusError(`could not save: ${errText(e)}`);
    return false;
  }
  // `null` is a cancelled panel: not a failure, not a save, and nothing to say.
  if (written === null || written === undefined) {
    status.textContent = "";
    return false;
  }
  markSaved();
  status.textContent = window.__opencalcNative
    ? `wrote a copy — ${written}`
    : `downloaded .${ext}`;
  return true;
}

/// The raw `.xlsx` path, kept because it is part of the host surface.
///
/// It used to call `session_save()` with no loss report at all, which is how an
/// `.xlsm` came back as an `.xlsx` with its macros silently gone (`IO-08`). It
/// is now the generic path with `xlsx` filled in, so there is one place the
/// question is asked.
export function doSave() {
  return doSaveFormat("xlsx");
}

export async function doSaveDelimited(delim, ext) {
  // Delimited text holds one sheet and no formatting. On a multi-sheet workbook
  // that is a lossy export chosen by someone who may not realise it, so it is
  // said before the download rather than after.
  //
  // **Not the generic `doSaveFormat` path**, and the reason is a feature:
  // `session_save_delimited` writes the sheet the user is *looking at*, which
  // `session_save_as("csv")` cannot express — it writes the first one. So this
  // keeps its own writer and its own sentence, which names that sheet, and asks
  // the engine for everything the sheet count does not cover.
  let sheets = 1;
  try { sheets = JSON.parse(wasm.session_sheet_names()).length; } catch {}
  let loss = "";
  try { loss = wasm.session_save_loss_for(ext); } catch {}
  if (sheets > 1 || loss) {
    const name = sheetNameAt(state.sheet);
    const others = sheets > 1
      ? `the other ${sheets - 1} sheet${sheets === 2 ? "" : "s"} and all formatting, formulas' styling and merges are not part of a ${ext.toUpperCase()} file`
      : `formatting, formulas' styling and merges are not part of a ${ext.toUpperCase()} file`;
    // The engine's own sentence, appended rather than replacing this one: it
    // knows about the macro project and this does not, and it does not know
    // which sheet is on screen.
    const ok = await confirmModal(
      `.${ext} holds one sheet`,
      `Only "${name}" will be written — ${others}.${loss ? ` And ${loss}.` : ""}`,
      `Export "${name}"`,
    );
    if (!ok) return false;
  }
  const text = wasm.session_save_delimited(state.sheet, delim);
  // A BOM so spreadsheet apps open it as UTF-8 rather than guessing a codepage
  // and turning every accented character into mojibake.
  //
  // The type comes from the engine: `text/csv` was written here for all three,
  // and it is the right answer for exactly one of them.
  download("\ufeff" + text, "opencalc." + ext, wasm.format_content_type(ext));
  // A delimited export holds one sheet and no formatting, so it is a save point
  // only in the sense that the user has the data — which is what the warning is
  // about. Marking it clean is the honest reading of "you have this on disk".
  markSaved();
  return true;
}

// --- The save target --------------------------------------------------------
//
// `docs/83` §2: **a document has one save target; `Ctrl+S` commits the document
// to that target and never creates a second document.** Phase A (`SAVE-02`)
// implements the `file` target — a path the desktop shell holds because a
// platform panel returned it.
//
// Before this, `Ctrl+S` in the desktop shell raised a Save As panel every time,
// so a user accumulated `opencalc (1).xlsx`, `opencalc (2).xlsx` beside the file
// they had opened, and the file they had opened was never updated. Downloading
// is not saving; `File ▸ Download` keeps doing what the keystroke used to.
//
// The shell decides nothing here beyond *where*: it holds the path, compares it
// against what it looked like when the document was opened, and writes through
// a temporary file and a rename so that a failed save never leaves the user
// with neither the old file nor the new one.

/// Commit `bytes` to the window's save target.
///
/// Three answers, because they are three different things for the caller to do:
/// `true` the bytes landed, `false` they did not and the user has been told,
/// and `"no-target"` — this document has never been saved, so the caller
/// *acquires* a target through the panel rather than downloading.
async function commitToTarget(bytes, force = false) {
  const native = window.__opencalcNative;
  let outcome;
  try {
    outcome = await native.saveTarget(bytes, force);
  } catch (e) {
    statusError(`could not save: ${errText(e)}`);
    return false;
  }
  const status_ = outcome && outcome.status;
  if (status_ === "written") {
    // **Only now.** `SAVE-01`'s lesson, and the reason this is awaited at all:
    // the document is marked saved by the completion of a write, never by the
    // start of one.
    markSaved();
    status.textContent = `saved ${outcome.name}`;
    return true;
  }
  if (status_ === "refused") {
    // A file that changed underneath us is a question, not a report — another
    // window, a sync client, or another application, and all three present
    // identically (`docs/83` §5.3–5.4). Detection rather than a lock file: a
    // stale lock strands a document nobody can then open.
    if (outcome.kind === "changed" && !force) {
      const ok = await confirmModal(
        `${outcome.name} changed on disk`,
        `Something else has written to ${outcome.name} since this window opened it — another window, another application, or a sync client. Saving now replaces what is there with this window's version, and what is there now cannot be brought back from here.`,
        "Overwrite",
      );
      if (!ok) {
        statusError(`not saved — ${outcome.name} changed on disk. File ▸ Download writes a copy under another name.`);
        return false;
      }
      return await commitToTarget(bytes, true);
    }
    statusError(`could not save: ${outcome.why}`);
    return false;
  }
  // `no-target`, and anything a future shell answers that this build does not
  // know: acquire a target rather than claim a save that did not happen.
  return "no-target";
}

/// The document's own format, to its own file.
///
/// `intent` is which command asked. `"save"` is `Ctrl+S` and commits to the
/// target, acquiring one through the panel when there is none. `"download"` is
/// `File ▸ Download ▸ Same format as opened`, which writes a copy and leaves
/// the target where it is.
export async function doSaveNative(intent = "save") {
  const ext = wasm.session_format();
  // Said before the write, because afterwards the file is already on disk.
  const loss = wasm.session_save_loss();
  if (loss) {
    const ok = await confirmModal(
      `.${ext} cannot carry all of this`,
      `${loss}. The file will hold everything a ${ext.toUpperCase()} file can, and nothing else.`,
      intent === "save" ? `Save .${ext}` : `Download .${ext}`,
    );
    if (!ok) return false;
  }
  const native = window.__opencalcNative;
  if (intent === "save" && native && native.saveTarget) {
    const done = await commitToTarget(wasm.session_save_native());
    // Anything but "no-target" is an answer; only a document that has never
    // been saved falls through to the panel below.
    if (done !== "no-target") return done;
  }
  // **`markSaved()` only after the bytes have landed.** It used to run
  // immediately after `download()`, which returns before the desktop shell has
  // even raised its panel — so a cancelled save, a failed write, or the boot
  // window where the shell still refuses everything cleared the dirty bullet
  // and disarmed the close warning while nothing had been written. The user
  // was told their work was safe and could then close the window (`SAVE-01`).
  //
  // `null` means the user cancelled, which is not a failure and not a save:
  // the document stays dirty and nothing is reported, because they know what
  // they did. A thrown error is a failure and is named.
  let written;
  try {
    written = await download(
      wasm.session_save_native(),
      `opencalc.${ext}`,
      wasm.session_format_content_type(),
      // Acquiring a target, not writing a copy: the file the user names in this
      // panel is where the document lives from now on, so the next Ctrl+S goes
      // straight there. A Download passes nothing and moves nothing.
      { adopt: intent === "save" },
    );
  } catch (e) {
    statusError(`could not save: ${errText(e)}`);
    return false;
  }
  if (written === null || written === undefined) {
    status.textContent = "";
    return false;
  }
  markSaved();
  // **A download is not a save, and the status must not claim otherwise.**
  // In a browser tab this put a file in the downloads folder and the document
  // has no home to go back to; only a host that owns a file has actually
  // *saved* anything. `docs/83` turns on that distinction, so the word here
  // has to keep it — and on the desktop the distinction is now between the
  // file the window commits to and a copy written beside it.
  if (!window.__opencalcNative) status.textContent = `downloaded .${ext}`;
  else status.textContent = intent === "save" ? `saved ${written}` : `wrote a copy — ${written}`;
  // A `Ctrl+S` that acquired a target is where the document lives now, so the
  // title bar has to say so — the shell adopted that path in the same call. A
  // Download did not, and neither did a browser tab: a file in the downloads
  // folder is a copy, and renaming the document to it would be the claim
  // `docs/83` §2 exists to stop.
  if (intent === "save" && window.__opencalcNative) openedName = written;
  return true;
}

/// The window is showing a different document now.
///
/// `File ▸ New` replaces the session outright, and the shell has to be told:
/// without this the next `Ctrl+S` writes a blank workbook over the file the
/// window was showing a moment ago. `docs/83` §3.2 names missing this clear as
/// the way a new document overwrites the last one.
///
/// The document name goes with it. It was already stale after a New — the
/// desktop title bar kept naming a file that was no longer open — and a stale
/// name is now also the thing the shell matches a save target against.
export function newDocument() {
  openedName = null;
  const native = window.__opencalcNative;
  if (native && native.clearSaveTarget) {
    native.clearSaveTarget().catch((err) => console.error("[opencalc] clearSaveTarget", err));
  }
}

/// The one entry point every save route goes through.
///
/// `fmt` is `"native"` — the document's own format — or any extension
/// [`writableFormats`] reports. It is no longer a list of four names this
/// function knows: `.ods` and `.xlsm` reach the engine because the engine says
/// it writes them, not because anybody remembered to add a branch.
///
/// `intent` distinguishes `Ctrl+S` from `File ▸ Download` and applies only to
/// `"native"`; every other format is a conversion, and a conversion is a
/// download whichever command asked for it.
export async function saveAs(fmt, intent = "save") {
  try {
    if (fmt === "native") { await doSaveNative(intent); return; }
    // Delimited text writes the sheet on screen, which the generic path cannot
    // express — see `doSaveDelimited`.
    const delim = fmt === "csv" ? 44 : fmt === "tsv" ? 9 : fmt === "psv" ? 124 : 0;
    if (delim) {
      if (await doSaveDelimited(delim, fmt)) status.textContent = "downloaded ." + fmt;
      return;
    }
    await doSaveFormat(fmt);
  } catch (e) { statusError(errText(e)); }
}

export function resetView() {
  state.scrollX = state.scrollY = 0;
  state.sel = { row: 0, col: 0 };
  state.anchor = { row: 0, col: 0 };
  endInline();
  draw();
}

export function saveSheetView() {
  const name = sheetNameAt(state.sheet);
  if (name == null) return;
  sheetViews.set(name, {
    scrollX: state.scrollX,
    scrollY: state.scrollY,
    sel: { ...state.sel },
    anchor: { ...state.anchor },
    selKind: state.selKind,
  });
}

export function switchSheet(i, keepEdit = false) {
  if (i === state.sheet) return;
  saveSheetView();
  state.sheet = i;
  // The map is per sheet.
  invalidateGrowth();
  if (!keepEdit) endInline();
  const v = sheetViews.get(sheetNameAt(i));
  if (v) {
    state.scrollX = v.scrollX;
    state.scrollY = v.scrollY;
    state.sel = { ...v.sel };
    state.anchor = { ...v.anchor };
    state.selKind = v.selKind;
    state.ranges = [];
    draw();
  } else {
    resetView(); // first visit to this sheet starts at A1
  }
  renderTabs();
  // Which participants are "elsewhere" is relative to the sheet on screen, so
  // the roster is re-read whenever that changes.
  renderPresence();
}

export function moveTab(from, to) {
  if (from < 0 || from === to) return;
  try { wasm.session_move_sheet(from, to); } catch (e) { statusError(errText(e)); return; }

  if (state.sheet === from) state.sheet = to;
  else {
    let a = state.sheet > from ? state.sheet - 1 : state.sheet;
    if (a >= to) a += 1;
    state.sheet = a;
  }
  renderTabs();
  draw();
}

export function renameSheet(i, tabEl) {
  if (!tabEl) return;
  const old = tabEl.textContent;
  const input = document.createElement("input");
  input.className = "sheet-rename";
  input.value = old;
  tabEl.textContent = "";
  tabEl.appendChild(input);
  input.focus();
  input.select();
  let done = false;
  const commit = () => {
    if (done) return;
    done = true;
    const name = input.value.trim();
    try { if (name && name !== old) wasm.session_rename_sheet(i, name); }
    catch (e) { statusError(errText(e)); }
    renderTabs();
  };
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") { commit(); e.preventDefault(); }
    else if (e.key === "Escape") { done = true; renderTabs(); }
  });
  input.addEventListener("blur", commit);
  input.addEventListener("click", (e) => e.stopPropagation());
  input.addEventListener("dblclick", (e) => e.stopPropagation());
}

/// The name of the document that is open, or null for one that never came
/// from a file.
///
/// The editor did not keep this: `openBytes` took a name and used it only to
/// choose a reader. In a browser tab nothing needed it — the tab title is the
/// application's. A desktop window's title bar is the document's, so the name
/// has to outlive the open that carried it.
let openedName = null;

/// What a title bar should call the current document.
export function documentName() {
  return openedName;
}

/// Rename the document (`UX-CHR-10`).
///
/// This is the *name*, not a file: nothing is written anywhere, and the next
/// Save As still asks. What it changes is what the strip says, what the desktop
/// window's title bar says, and what a later save offers as the default — which
/// is the whole of what a rename means before there is a file on disk.
///
/// An empty or whitespace-only name is refused rather than stored. A document
/// called "" reads as untitled everywhere it is shown, so storing it would be a
/// state the user cannot see and cannot get out of except by renaming again.
export function setDocumentName(name) {
  const trimmed = (name ?? "").trim();
  if (!trimmed) return false;
  openedName = trimmed;
  return true;
}

export function openBytes(raw, name = "workbook.xlsx", budgetMs = undefined) {
  let bytes = raw;
  const dot = name.lastIndexOf(".");
  // No extension at all means a package — `openBytes(bytes)` names nothing and
  // that is what it has always meant. An extension the engine does *not* know
  // is a different thing and is refused below rather than guessed at: guessing
  // is how a file gets opened as a package and saved back under its own name.
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "xlsx";
  // Which format an extension names is the SDK's answer, asked for here rather
  // than copied into a table of the page's own. The copy is the defect
  // (`WASM-01`): it opened exactly the formats it was last told about, so a
  // `.csv` was openable through the WOPI service and not in the editor.
  let canonical = "";
  let isText = false;
  try {
    canonical = wasm.format_for_extension(ext);
    isText = !!canonical && wasm.format_is_text(ext);
  } catch {}
  let ok = true;
  let stopped = false;
  clearKeepWaiting();
  try {
    stopMarch();
    if (!canonical) throw new Error(`.${ext} is not a format this build can open`);
    // Delimited text carries no encoding declaration, so it is decoded here; a
    // package declares its own and must be handed over byte for byte.
    if (isText) bytes = decodeTextBytes(bytes);
    // A wall-clock limit, so a workbook that is inside every admission bound
    // and simply enormous cannot hold the tab's one thread until it finishes
    // (`SEC-017`). Cleared afterwards: the budget bounds this job, not the
    // next one.
    wasm.session_set_time_budget_ms(budgetMs === undefined ? openBudgetMs : budgetMs);
    try {
      wasm.session_open_as(ext, bytes);
    } finally {
      wasm.session_clear_time_budget();
    }
    status.textContent = `opened ${name}`;
    reportImportIssues();
  } catch (err) {
    status.textContent = friendlyOpenError(err, name, isText);
    stopped = wasStopped(err);
    ok = false;
  }
  invalidateGrowth();
  // Part paths repeat across workbooks — every file has an
  // `xl/media/image1.png` — so a cache kept across a load shows the previous
  // file's pictures.
  imageCache.clear();
  syncClock();
  // Open on the sheet the file was left on, not always the first: a workbook
  // records which tab its author was looking at, and a summary sheet at the
  // end is put there deliberately.
  try { state.sheet = wasm.session_active_sheet(); } catch { state.sheet = 0; }
  state.scrollX = state.scrollY = 0;
  renderTabs();
  select(0, 0);
  // After the bookkeeping, so the offer is appended to the message that is
  // actually left standing. A stopped open loaded nothing, so what is on screen
  // is still whole — this hands back the choice the limit took, rather than
  // leaving a big file permanently unopenable.
  if (stopped) offerKeepWaiting(`opening ${name}`, () => openBytes(raw, name, -1));
  // Only a successful open renames the window. A failed one leaves the
  // previous document's name in place, because the previous document is what
  // is still on screen.
  if (ok) openedName = name;
  return ok;
}

export function setZoomForTest(z) {
  setZoom(z);
}

export function clearMyViewForTest() {
  clearMyView();
}
