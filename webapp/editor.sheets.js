// Sheet-level and document-level commands: tabs, saving, printing,
// protection and personal views.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  A1,
  HH,
  HW,
  ZOOM_MAX,
  ZOOM_MIN,
  afterFilterChange,
  allRanges,
  applyCommandRules,
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
  tryEdit,
  wasStopped,
  wasm,
} from "./editor.core.js";

export function setZoom(z) {
  const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z * 100) / 100));
  if (next === state.zoom) return;
  state.zoom = next;
  // Text is measured in zoom-logical units, so every grown row's height changes.
  clearHeightMemo();
  invalidateGrowth();
  resize();
  status.textContent = `zoom ${Math.round(next * 100)}%`;
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

export function doSave() {
  download(
    wasm.session_save(),
    "opencalc.xlsx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  );
  markSaved();
}

export async function doSaveDelimited(delim, ext) {
  // Delimited text holds one sheet and no formatting. On a multi-sheet workbook
  // that is a lossy export chosen by someone who may not realise it, so it is
  // said before the download rather than after.
  let sheets = 1;
  try { sheets = JSON.parse(wasm.session_sheet_names()).length; } catch {}
  if (sheets > 1) {
    const name = sheetNameAt(state.sheet);
    const ok = await confirmModal(
      `.${ext} holds one sheet`,
      `Only "${name}" will be written — the other ${sheets - 1} sheet${sheets === 2 ? "" : "s"} and all formatting, formulas' styling and merges are not part of a ${ext.toUpperCase()} file.`,
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

export async function doSaveNative() {
  const ext = wasm.session_format();
  // Said before the download, because afterwards the file is already on disk.
  const loss = wasm.session_save_loss();
  if (loss) {
    const ok = await confirmModal(
      `.${ext} cannot carry all of this`,
      `${loss}. The download will hold everything a ${ext.toUpperCase()} file can, and nothing else.`,
      `Download .${ext}`,
    );
    if (!ok) return false;
  }
  download(wasm.session_save_native(), `opencalc.${ext}`, wasm.session_format_content_type());
  markSaved();
  status.textContent = "downloaded ." + ext;
  return true;
}

export async function saveAs(fmt) {
  try {
    if (fmt === "native") { await doSaveNative(); return; }
    if (fmt === "xlsx") { doSave(); status.textContent = "downloaded .xlsx"; return; }
    const delim = fmt === "csv" ? 44 : fmt === "tsv" ? 9 : 124;
    if (await doSaveDelimited(delim, fmt)) status.textContent = "downloaded ." + fmt;
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
