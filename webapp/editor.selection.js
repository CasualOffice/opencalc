// Selection, editing, the formula bar, autofill and the command registry.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  A1,
  A1_PART,
  markSaved,
  HH,
  HW,
  ROW_H,
  acEl,
  acState,
  activeEl,
  applyPersonalFilter,
  beginEdit,
  byId,
  calcMode,
  canvas,
  capabilityForbids,
  cellFont,
  cellPx,
  cellRef,
  colName,
  colWAt,
  colXAt,
  commandRules,
  confirmModal,
  ctx,
  draw,
  editHome,
  editMode,
  editOriginal,
  editSurface,
  effectiveRange,
  endEdit,
  ensureVisible,
  enterStep,
  errText,
  extend,
  extending,
  fInput,
  filterHidden,
  filterInfo,
  fmtNum,
  formulaRefDrag,
  friendlyFormulaError,
  fscreenX,
  fscreenXEnd,
  fscreenY,
  fscreenYEnd,
  getCapabilities,
  hiddenColMarks,
  hiddenRowMarks,
  hideAutocomplete,
  inline,
  invalidateGrowth,
  isReadOnlySafe,
  lastReported,
  listeners,
  measureRowHeight,
  mergeAt,
  mirrorEdit,
  modeEl,
  needsRecalc,
  parseNameRange,
  pointMode,
  qs,
  qsa,
  readOnly,
  refSpans,
  renderTabs,
  resetView,
  rowHAt,
  rowYAt,
  selRect,
  selStats,
  select,
  setTip,
  sheetMerges,
  sheetNameAt,
  start,
  state,
  status,
  statusError,
  switchSheet,
  t,
  updateRefSpans,
  updateSignatureTip,
  usedBounds,
  wasm,
  wrap,
} from "./editor.core.js";

export function updateStats() {
  if (!wasm) return;
  const rs = allRanges();
  const single = rs.length === 1;
  const s = rs[0];
  if (single && s.r0 === s.r1 && s.c0 === s.c1) { selStats.textContent = ""; return; }
  // Fold the per-range stats together (disjoint Ctrl+click ranges; overlaps,
  // which Excel also double-counts, are rare).
  let sum = 0, numeric = 0, count = 0;
  for (const r of rs) {
    const st = JSON.parse(wasm.session_range_stats(state.sheet, r.r0, r.c0, r.r1, r.c1));
    sum += st.sum || 0; numeric += st.numeric || 0; count += st.count || 0;
  }
  // **Excel's default three, in Excel's order and Excel's wording**
  // (`UX-CHR-04`, `docs/88` §5).
  //
  // This showed six — `Sum · Avg · Min · Max · Numbers · Count` — in a frosted
  // panel over the cells it was describing. Six is more than any competitor's
  // default bar OnlyOffice's five, and none of them floats it. Now that the
  // summary is text in the status bar it shares that bar with the sheet tabs,
  // the mode indicator, the engine line and zoom, so what it shows has to be
  // what a user asked for: Excel shows `Average: 3   Count: 5   Sum: 15` and
  // adds the rest by right-clicking the bar, which is the follow-on `docs/88`
  // §5 names and `UX-MOB-07`'s narrow-bar question will meet.
  //
  // Min and Max were folded here and are not any more: `session_range_stats`
  // returns them in the same call, so the right-click menu `docs/88` §5 names
  // can read them when it is built, and a fold whose result nothing draws is
  // dead arithmetic in a function that runs on every drag-select frame.
  const parts = [];
  if (numeric > 0) parts.push(`Average: <b>${fmtNum(sum / numeric)}</b>`);
  parts.push(`Count: <b>${count}</b>`);
  if (numeric > 0) parts.push(`Sum: <b>${fmtNum(sum)}</b>`);
  // oc-safe-html: every part is a number this function computed, joined
  // with non-breaking spaces. No workbook text reaches here.
  // oc-safe-html: see the note above.
  selStats.innerHTML = parts.join("&nbsp;&nbsp;");
}

// --- What the whole selection carries, not just its corner -----------------

// How many cells the toolbar will read before it answers "indeterminate"
// instead of an answer it has not checked.
//
// This is a per-*frame* budget, not a per-click one: `refreshFormulaBar` runs
// from `draw()`, which the copy marquee re-runs every animation frame and a
// drag-select re-runs on every mouse move. 1024 is a 32x32 block — larger than
// the selections people make by hand to format something, and ~1 ms of engine
// calls in the worst case (a uniform 1024-cell block, where every cell must be
// read before "uniform" can be claimed). The two cheap paths below mean the
// worst case is rare: an identical serialization is compared as a string
// without parsing, and a selection that has already disagreed about everything
// stops early.
//
// Past the cap the answer is "not checked", rendered exactly like "mixed" —
// blank boxes, unpressed toggles. Reporting the top-left cell for a range it
// was never compared against is the defect this replaces, and a sampled answer
// would be the same lie with more steps. Whole-row / -column /
// select-all selections are clipped to `usedBounds()` by `effectiveRange()`,
// so on an ordinary sheet they are usually under the cap and do get checked.
const UNIFORM_SCAN_CAP = 1024;

// The toolbar-visible slice of `session_cell_format`, flattened so two cells
// can be compared field by field with `!==`. Keys are the engine's short ones
// (`fn` font name, `fs` size, `nf` number format, `bg` fill, `fc` text colour)
// plus `ov`, which is the *button's* state: #tb-wrap lights for wrap or clip.
function toolbarStyle(raw) {
  return {
    fn: raw.fn || "",
    fs: raw.fs ? String(raw.fs) : "",
    b: !!raw.b,
    i: !!raw.i,
    u: !!raw.u,
    st: !!raw.st,
    ov: !!(raw.w || raw.cl),
    al: raw.al || "",
    nf: raw.nf || "",
    bg: raw.bg || "",
    fc: raw.fc || "",
  };
}

const NO_KEYS = new Set();

// The placeholders the markup ships ("Default" / "11"), captured before the
// first time a mixed selection overwrites them.
const basePlaceholder = new Map();

// The formatting every selected cell agrees on, plus the set of properties they
// do not. `style` is the representative (top-left) cell's — valid only for the
// keys absent from `mixed`.
function selectionStyle() {
  const pr = selRect();
  const headRaw = wasm.session_cell_format(state.sheet, pr.r0, pr.c0);
  const head = toolbarStyle(JSON.parse(headRaw));
  const keys = Object.keys(head);
  // `allRanges()` is exactly the set of cells a toolbar command would write
  // (`formatSel` iterates it), so the toolbar describes what it would change.
  // A plain single-block selection is that set already, and taking it directly
  // keeps the common case off `effectiveRange` → `usedBounds`, which is an
  // engine call this runs on every frame of a drag.
  const ranges = state.ranges.length === 0 && state.selKind === "cells"
    ? [pr]
    : allRanges();
  let cells = 0;
  for (const g of ranges) cells += (g.r1 - g.r0 + 1) * (g.c1 - g.c0 + 1);
  if (cells <= 1) return { style: head, mixed: NO_KEYS, cells, scanned: true };
  if (cells > UNIFORM_SCAN_CAP) {
    return { style: head, mixed: new Set(keys), cells, scanned: false };
  }
  const mixed = new Set();
  scan:
  for (const g of ranges) {
    for (let row = g.r0; row <= g.r1; row++) {
      for (let col = g.c0; col <= g.c1; col++) {
        if (row === pr.r0 && col === pr.c0) continue;
        const raw = wasm.session_cell_format(state.sheet, row, col);
        // An identical serialization is identical formatting: the engine emits
        // these keys in a fixed order and only when set, so the uniform case
        // costs one string compare and no parse. The converse does not hold —
        // `qp` and the border edges are in the same string and the toolbar does
        // not mirror them — so a *differing* string still has to be compared
        // field by field rather than declared mixed.
        if (raw === headRaw) continue;
        const s = toolbarStyle(JSON.parse(raw));
        for (const k of keys) if (s[k] !== head[k]) mixed.add(k);
        // Nothing left to learn; the rest of the range cannot change the answer.
        if (mixed.size === keys.length) break scan;
      }
    }
  }
  return { style: head, mixed, cells, scanned: true };
}

// Menu labels for number-format codes, read from #numfmt-menu so the toolbar
// names a format the same way the menu that applies it does. Built once: the
// menu is static markup.
let numFmtLabels = null;
function numberFormatLabel(code) {
  if (!numFmtLabels) {
    numFmtLabels = new Map();
    for (const b of qsa("#numfmt-menu button[data-nf]")) {
      if (b.dataset.nf !== "__custom__") {
        numFmtLabels.set(b.dataset.nf, (b.textContent || "").trim());
      }
    }
  }
  // A format applied from the cell-format dialog need not be in the menu; its
  // own code is then the most honest label available.
  return numFmtLabels.get(code) || code || "Automatic (General)";
}

export function refreshFormulaBar() {
  if (state.editing) return;
  // Don't clobber a control the user is actively typing in. Background redraws
  // (e.g. the marching-ants copy animation) call this every frame; without this
  // guard they'd reset the formula bar / font / size boxes mid-keystroke.
  const active = activeEl();
  if (active === fInput || active === byId("tb-font") ||
      active === byId("tb-size")) return;
  fInput.value = wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  // Name what each will do, rather than leaving "Undo" to mean anything. The
  // label is the engine's, so it always matches the operation on the stack.
  for (const [id, can, label, verb, key] of [
    ["tb-undo", "session_can_undo", "session_undo_label", "Undo", "Ctrl+Z"],
    ["tb-redo", "session_can_redo", "session_redo_label", "Redo", "Ctrl+Shift+Z"],
  ]) {
    const btn = byId(id);
    const enabled = wasm[can]();
    btn.disabled = !enabled;
    let what = "";
    try { what = enabled ? wasm[label]() : ""; } catch {}
    const text = what ? `${verb} ${what} (${key})` : `${verb} (${key})`;
    setTip(btn, text);
  }
  // Reflect the formatting of the *selection*, using its top-left (the
  // representative/active cell) as the answer only where every selected cell
  // agrees with it. For a range/row/column selection state.sel is the *moving
  // end*, which is often an empty corner — reading that left the boxes blank.
  let sel;
  try { sel = selectionStyle(); }
  catch { sel = { style: toolbarStyle({}), mixed: new Set(), cells: 1, scanned: true }; }
  const f = sel.style;
  const mixed = (k) => sel.mixed.has(k);
  // `aria-pressed="mixed"` is the ARIA state for a toggle that is neither on
  // nor off, which is exactly what a disagreeing selection is.
  const press = (id, on, key) =>
    byId(id).setAttribute("aria-pressed", mixed(key) ? "mixed" : on ? "true" : "false");
  press("tb-bold", f.b, "b");
  press("tb-italic", f.i, "i");
  press("tb-underline", f.u, "u");
  press("tb-strike", f.st, "st");
  press("tb-wrap", f.ov, "ov");
  // The three one-click number formats light when the selection already carries
  // exactly the code that button applies — the same codes `toggleFormat` in
  // editor.core.js compares against to toggle them back off, so the lit state
  // and the second press agree about what "already applied" means.
  press("tb-currency", f.nf === "$#,##0.00", "nf");
  press("tb-percent", f.nf === "0%", "nf");
  press("tb-comma", f.nf === "#,##0.00", "nf");
  // Alignment is a radio group, not three toggles: a mixed selection lights
  // none of them rather than marking all three indeterminate.
  for (const b of qsa(".tb-align")) {
    b.setAttribute(
      "aria-pressed",
      !mixed("al") && b.dataset.al === f.al ? "true" : "false",
    );
  }
  // Everything else a cell can be formatted with lives behind #tb-numfmt, so
  // that button carries the rest: an accent underline when the selection has a
  // format at all (a date, an accounting code, a custom one — none of which has
  // a button), the format's own name in its tooltip, and a check beside it in
  // the menu. No text on the button: it is a 30px pill and a readout inside it
  // would resize the toolbar group.
  const nfBtn = byId("tb-numfmt");
  nfBtn.classList.toggle("tb-mixed", mixed("nf"));
  nfBtn.classList.toggle("has-nf", !mixed("nf") && !!f.nf);
  setTip(
    nfBtn,
    mixed("nf")
      ? sel.scanned
        ? "Number format — mixed across the selection"
        : `Number format — not checked (${sel.cells} cells selected)`
      : `Number format — ${numberFormatLabel(f.nf)}`,
  );
  for (const b of qsa("#numfmt-menu button[data-nf]")) {
    const on = !mixed("nf") && b.dataset.nf !== "__custom__" && b.dataset.nf === f.nf;
    b.classList.toggle("checked", on);
    if (on) b.setAttribute("aria-current", "true");
    else b.removeAttribute("aria-current");
  }
  // Fill and text colour, mirrored onto the buttons the same way #tb-border
  // mirrors the chosen line colour: a CSS variable feeding a bar under the
  // glyph (see `syncBorderPicks` in editor.core.js and `#tb-border::after` in
  // editor.css). One mechanism, not two.
  const paint = (id, key, prop, value, tip) => {
    const btn = byId(id);
    btn.classList.toggle("tb-mixed", mixed(key));
    // Overwritten on the mixed path too, rather than just hatched over: a
    // variable left holding the last selection's colour is a stale answer
    // waiting for the class to come off.
    btn.style.setProperty(prop, mixed(key) ? "transparent" : value);
    setTip(btn, tip);
  };
  const colorTip = (label, key, hex, none) =>
    mixed(key)
      ? sel.scanned
        ? `${label} — mixed across the selection`
        : `${label} — not checked (${sel.cells} cells selected)`
      : hex
        ? `${label} — #${hex}`
        : `${label} — ${none}`;
  // No explicit text colour means the cell paints in the sheet's own text
  // colour, so that is what "automatic" shows.
  paint("tb-fontcolor", "fc", "--oc-x-font-swatch",
    f.fc ? "#" + f.fc : "var(--oc-text-color)",
    colorTip("Text color", "fc", f.fc, "automatic"));
  // No fill leaves the bar empty rather than painting the page colour, which
  // would read as "filled white".
  paint("tb-fillcolor", "bg", "--oc-x-fill-swatch",
    f.bg ? "#" + f.bg : "transparent",
    colorTip("Fill color", "bg", f.bg, "none"));
  // Font name and size: blank when the selection disagrees, as Excel does.
  // Showing the top-left cell's 14 for a 14/9 selection is how somebody applies
  // 14pt to a selection they had been told was already 14pt.
  const combo = (id, key, value, base) => {
    const box = byId(id);
    if (basePlaceholder.get(id) === undefined) basePlaceholder.set(id, box.placeholder);
    box.value = mixed(key) ? "" : value;
    box.placeholder = mixed(key) ? "—" : basePlaceholder.get(id);
    setTip(
      box,
      mixed(key)
        ? sel.scanned
          ? `${base} — mixed across the selection`
          : `${base} — not checked (${sel.cells} cells selected)`
        : base,
    );
  };
  combo("tb-font", "fn", f.fn, "Font (type any font)");
  combo("tb-size", "fs", f.fs, "Font size (type any size)");
}

export function stepWithin(b, axis, back) {
  let { row, col } = state.sel;
  const d = back ? -1 : 1;
  if (axis === "row") {
    row += d;
    if (row > b.r1) { row = b.r0; col = col + 1 > b.c1 ? b.c0 : col + 1; }
    else if (row < b.r0) { row = b.r1; col = col - 1 < b.c0 ? b.c1 : col - 1; }
  } else {
    col += d;
    if (col > b.c1) { col = b.c0; row = row + 1 > b.r1 ? b.r0 : row + 1; }
    else if (col < b.c0) { col = b.c1; row = row - 1 < b.r0 ? b.r1 : row - 1; }
  }
  state.sel = { row, col };
  ensureVisible();
  draw();
}

export function stepFrom(row, col, dr, dc) {
  const m = mergeAt(row, col);
  if (!m) return { row: row + dr, col: col + dc };
  // Leave from the edge facing the way we are going, so the landing cell is the
  // first one outside the merge rather than one still inside it.
  const fromRow = dr > 0 ? m.r1 : dr < 0 ? m.r0 : row;
  const fromCol = dc > 0 ? m.c1 : dc < 0 ? m.c0 : col;
  return { row: fromRow + dr, col: fromCol + dc };
}

export function addRange(row, col) {
  // Ctrl+clicking a cell that is already selected *removes* it, as in Excel —
  // otherwise a mis-click into a multi-range selection could only be undone by
  // starting the whole selection again.
  const hit = state.ranges.findIndex(
    (g) => row >= g.r0 && row <= g.r1 && col >= g.c0 && col <= g.c1,
  );
  if (hit >= 0) {
    state.ranges = state.ranges.filter((_, i) => i !== hit);
    draw();
    return;
  }
  state.ranges = state.ranges.concat([effectiveRange()]);
  let r = Math.max(0, row), c = Math.max(0, col);
  const m = mergeAt(r, c);
  if (m) { r = m.r0; c = m.c0; }
  state.sel = { row: r, col: c };
  state.anchor = { row: r, col: c };
  state.selKind = "cells";
  draw();
}

export function addColumnRange(c) {
  state.ranges = state.ranges.concat([effectiveRange()]);
  state.selKind = "cols";
  state.anchor = { row: 0, col: c };
  state.sel = { row: 0, col: c };
  endInline();
  draw();
}

export function addRowRange(r) {
  state.ranges = state.ranges.concat([effectiveRange()]);
  state.selKind = "rows";
  state.anchor = { row: r, col: 0 };
  state.sel = { row: r, col: 0 };
  endInline();
  draw();
}

// Excel's Ctrl+Enter: the entry lands in every selected cell at once, and the
// selection survives it. Relative references adjust per cell exactly as a fill
// would — typing `=A1*2` across B1:B3 gives `=A2*2` in B2, not three copies of
// the first — which is why this goes through `session_fill` rather than writing
// the same text N times. One engine call is also one undo step: N writes would
// need N presses of Ctrl+Z to take back a single gesture.
//
// It routes through `commit` first so the entry meets the same validation rule
// and formula guard as any other typed value. A refused entry fills nothing.
export function commitToSelection(value) {
  const r = effectiveRange();
  // Captured before committing, because commit moves the selection and the
  // block being filled is the one that was selected when the user pressed.
  const anchor = { ...state.anchor };
  const sel = { ...state.sel };
  if (!commit(value, false)) return false;
  if (r.r0 === r.r1 && r.c0 === r.c1) return true;
  try {
    // Source is the cell just written; it sits inside the destination, which is
    // safe because the engine resolves every source before it writes anything.
    wasm.session_fill(state.sheet, sel.row, sel.col, sel.row, sel.col, r.r0, r.c0, r.r1, r.c1);
    state.anchor = anchor;
    state.sel = sel;
    status.textContent = `filled ${(r.r1 - r.r0 + 1) * (r.c1 - r.c0 + 1)} cells`;
  } catch (e) { statusError(errText(e)); }
  draw();
  return true;
}

export function commit(value, advance, source = "user") {
  // Cancellable before it is written, and told who is writing. A host without
  // either cannot enforce its own permissions, and cannot tell its own
  // programmatic write from a keystroke — so it echoes its change back into its
  // own store and loops.
  const at = editHome ?? { sheet: state.sheet, row: state.sel.row, col: state.sel.col };
  if (!emit("beforeCellsChanged", {
    sheet: at.sheet,
    range: { r0: at.row, c0: at.col, r1: at.row, c1: at.col },
    value,
    source,
  })) {
    statusError("that change was refused by the application");
    if (editSurface) { editSurface.classList.add("invalid"); editSurface.focus({ preventScroll: true }); }
    return false;
  }
  // A cross-sheet pick leaves the view on another sheet; go back before writing,
  // or the value would land on whichever sheet the user happened to end on.
  if (editHome && editHome.sheet !== state.sheet) {
    switchSheet(editHome.sheet, true);
    state.sel = { row: editHome.row, col: editHome.col };
    state.anchor = { ...state.sel };
  }
  // A cell under a data-validation rule refuses input the rule disallows. The
  // dropdown was previously a suggestion — anything typed over it was accepted,
  // which is the opposite of what a validation is for. Gated on typed entry
  // only, as in Excel: fill and paste are not checked.
  let advisory = "";
  if (!value.trim().startsWith("=")) {
    let bad = "";
    // The `catch` is belt and braces, not a decision: `session_validation_error`
    // returns a `String` rather than a `Result`, so it has no ordinary failure
    // to report and this cannot swallow a refusal. Reported as a fail-open
    // integrity hole and checked before being believed — the binding's return
    // type is the answer.
    try { bad = wasm.session_validation_error(state.sheet, state.sel.row, state.sel.col, value); }
    catch {}
    if (bad) {
      let alert = { style: "stop", title: "", text: bad };
      try { alert = JSON.parse(bad); } catch {}
      const label = alert.title ? `${alert.title} — ${alert.text}` : alert.text;
      // Only `stop` refuses. `warning` lets the value through after a
      // confirmation and `information` merely says so — the author chose which,
      // and blocking all three leaves no way past an advisory rule.
      if (alert.style === "stop") {
        statusError(`Not allowed here — ${label}`);
        if (editSurface) { editSurface.classList.add("invalid"); editSurface.focus({ preventScroll: true }); }
        return false;
      }
      // `warning` and `information` both let the value through. Excel asks
      // "continue?" for a warning; this reports it instead, because the commit
      // path is synchronous and cannot await a dialog. Accepting with a visible
      // message is the same outcome as answering yes, and the edit is undoable.
      //
      // Held until after the commit: the success path ends with "ok", which
      // would otherwise wipe the only sign the value broke a rule.
      advisory = label;
    }
  }
  // Reject an unparseable formula instead of silently storing it as text —
  // keep the editor open with the error, like Excel's formula guard.
  if (value.trim().startsWith("=")) {
    let err = "";
    try { err = wasm.validate_formula(value); } catch {}
    if (err) {
      // Refuse the commit whether the edit came from the grid or the formula
      // bar — never silently store an unparseable formula as literal text. The
      // red outline and the refocus land on whichever surface was being typed
      // in, so the formula bar reports its own errors like the cell does.
      statusError(`Formula error: ${friendlyFormulaError(err)}`);
      if (editSurface) { editSurface.classList.add("invalid"); editSurface.focus({ preventScroll: true }); }
      return false;
    }
  }
  try {
    wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, value);
    // A value with a hard line break is only legible with wrap on, so entering
    // one turns wrap on for that cell — as Excel does for Alt+Enter.
    if (value.includes("\n")) {
      const fmt = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col));
      if (!fmt.w) {
        wasm.session_toggle_wrap(state.sheet, state.sel.row, state.sel.col, state.sel.row, state.sel.col);
      }
    }
    // A table grows to take in a value typed just below or beside it. Done
    // after the value lands so the engine sees the cell it is growing for.
    //
    // Swallowed deliberately: the value is already committed, and this call is
    // a probe as much as an edit — it is made after *every* commit, and the
    // common answer is "that cell borders no table". Reporting that would put
    // an error on the status bar for an edit that succeeded.
    try { wasm.session_table_autoexpand(state.sheet, state.sel.row, state.sel.col); } catch {}
    if (advisory) statusError(advisory);
    else status.textContent = "ok";
  } catch (e) {
    // A refused edit keeps the editor open on its own cell. Advancing would
    // move the cursor away from the thing that just failed and leave the
    // message pointing at a cell the user is no longer looking at — which is
    // how a protected sheet appeared to accept the value.
    statusError(errText(e));
    if (editSurface) { editSurface.classList.add("invalid"); editSurface.focus({ preventScroll: true }); }
    return false;
  }
  emit("cellsChanged", {
    sheet: at.sheet,
    range: { r0: at.row, c0: at.col, r1: at.row, c1: at.col },
    value,
    source,
  });
  endEdit();
  // Move to the next row on Enter as a fresh single-cell selection (reset the
  // anchor + clear any multi-range, else anchor stays put and paints a ghost
  // 2-cell range).
  // Enter after an edit obeys the same rules as Enter on the grid: walk the
  // block if one is being walked, else return to the Tab run's column.
  if (advance) enterStep(false);
  else { ensureVisible(); draw(); }
  return true;
}

export function selectAll() {
  state.selKind = "all";
  state.ranges = [];
  state.anchor = { row: state.firstRow, col: state.firstCol };
  state.sel = { row: state.firstRow, col: state.firstCol };
  endInline();
  draw();
}

export function ctrlA() {
  const r = selRect();
  const covers = (r0, c0, r1, c1) =>
    state.selKind === "cells" && r.r0 === r0 && r.c0 === c0 && r.r1 === r1 && r.c1 === c1;

  const select = (r0, c0, r1, c1) => {
    state.selKind = "cells";
    state.ranges = [];
    state.anchor = { row: r0, col: c0 };
    state.sel = { row: r1, col: c1 };
    endInline();
    draw();
  };

  let block = null;
  if (wasm) {
    try {
      const j = wasm.session_block_bounds(state.sheet, state.sel.row, state.sel.col);
      if (j && j !== "null") block = JSON.parse(j);
    } catch {}
  }
  const b = usedBounds();
  const usedIsBlock =
    block && block.r0 === 0 && block.c0 === 0 &&
    block.r1 === b.rows - 1 && block.c1 === b.cols - 1;

  if (block && !covers(block.r0, block.c0, block.r1, block.c1)) {
    select(block.r0, block.c0, block.r1, block.c1);
    return;
  }
  // A block that already *is* the used region has no wider step to take, so it
  // goes straight to the sheet rather than re-selecting itself.
  if (!usedIsBlock && !covers(0, 0, b.rows - 1, b.cols - 1)) {
    select(0, 0, b.rows - 1, b.cols - 1);
    return;
  }
  selectAll();
}

export function selectRow(r, exp) {
  state.selKind = "rows";
  if (!exp) { state.anchor = { row: r, col: 0 }; state.ranges = []; }
  state.sel = { row: r, col: 0 };
  endInline();
  draw();
}

export function selectColumn(c, exp) {
  state.selKind = "cols";
  if (!exp) { state.anchor = { row: 0, col: c }; state.ranges = []; }
  state.sel = { row: 0, col: c };
  endInline();
  draw();
}

export function selectRowsSpan() {
  const r = selRect();
  state.selKind = "rows";
  state.anchor = { row: r.r0, col: 0 };
  state.sel = { row: r.r1, col: 0 };
  state.ranges = [];
  endInline();
  draw();
}

export function selectColsSpan() {
  const r = selRect();
  state.selKind = "cols";
  state.anchor = { row: 0, col: r.c0 };
  state.sel = { row: 0, col: r.c1 };
  state.ranges = [];
  endInline();
  draw();
}

export function allRanges() {
  return state.ranges.concat([effectiveRange()]);
}

export function autofitColumn(col) {
  const b = usedBounds();
  const items = JSON.parse(wasm.session_cells(state.sheet, 0, col, b.rows - 1, col));
  // Read fresh rather than trusting `sheetMerges`, which is refreshed on draw:
  // autofit can run against a sheet edited since the last one.
  const merges = JSON.parse(wasm.session_merges(state.sheet));
  const spansColumns = (row) =>
    merges.some((m) => row >= m.r0 && row <= m.r1 && col >= m.c0 && col <= m.c1 && m.c1 > m.c0);
  let maxw = 24;
  for (const it of items) {
    if (!it.t) continue;
    // A cell merged across columns cannot size one of them. Its text is as wide
    // as the whole span, so charging it to a single column makes that column as
    // wide as the title above the table — which is what "naive" meant here, and
    // it is why Excel leaves merged cells out of autofit rather than trying to
    // apportion them.
    if (spansColumns(it.r)) continue;
    ctx.font = cellFont(it);
    const flat = ctx.measureText(String(it.t)).width;
    // Rotated text needs *less* width, not more: what the column has to hold is
    // the run projected onto the horizontal axis. Sizing to the flat width
    // leaves a column several times wider than the text in it, which is the
    // opposite of what autofit is for.
    if (it.rot === 255) {
      // Stacked: one glyph per line, so the width is the widest single glyph.
      let widest = 0;
      for (const ch of String(it.t)) widest = Math.max(widest, ctx.measureText(ch).width);
      maxw = Math.max(maxw, widest);
    } else if (it.rot) {
      const deg = it.rot <= 90 ? it.rot : it.rot - 90;
      maxw = Math.max(maxw, Math.abs(Math.cos((deg * Math.PI) / 180)) * flat + cellPx(it));
    } else {
      maxw = Math.max(maxw, flat);
    }
  }
  // Said out loud, not swallowed: autofit is reached by double-clicking a
  // column boundary, and a protected sheet refuses the width. A refusal nobody
  // sees is a boundary you double-click again, harder.
  try { wasm.session_set_col_width(state.sheet, col, Math.ceil(maxw) + 14); }
  catch (e) { statusError(errText(e)); }
  draw();
}

export function autofitRow(row) {
  const b = usedBounds();
  const items = JSON.parse(wasm.session_cells(state.sheet, row, 0, row, b.cols - 1));
  const merges = JSON.parse(wasm.session_merges(state.sheet));
  const spansRows = (col) =>
    merges.some((m) => row >= m.r0 && row <= m.r1 && col >= m.c0 && col <= m.c1 && m.r1 > m.r0);
  let maxh = ROW_H;
  for (const it of items) {
    if (!it.t) continue;
    // The same rule the other way round: text in a cell merged down several
    // rows is as tall as the span, and giving one row all of that height
    // over-sizes it by however many rows it shares with.
    if (spansRows(it.c)) continue;
    // `measureRowHeight` rather than a copy of its arithmetic. This used to
    // reimplement it — under a comment saying to match it exactly — and the
    // copy had no rotation case, so autofitting a row of rotated headings sized
    // it as though the text were flat and clipped every one of them. Since
    // autofit *persists* the height, and a persisted height pins the row
    // against further auto-growth, that was not self-correcting.
    maxh = Math.max(maxh, measureRowHeight(it, colWAt(it.c)) ?? ROW_H);
  }
  try { wasm.session_set_row_height(state.sheet, row, Math.ceil(maxh)); }
  catch (e) { statusError(errText(e)); }
  draw();
}

export function parseColor(input) {
  const v = String(input || "").trim().toLowerCase();
  if (!v) return null;
  const hex = v.replace(/^#/, "");
  if (/^[0-9a-f]{3}$/.test(hex)) {
    // Shorthand doubles each nibble: #abc is #aabbcc.
    return [...hex].map((c) => c + c).join("").toUpperCase();
  }
  if (/^[0-9a-f]{6}$/.test(hex)) return hex.toUpperCase();
  const to255 = (n) => Math.max(0, Math.min(255, Math.round(n)));
  const hx = (n) => to255(n).toString(16).padStart(2, "0").toUpperCase();
  let m = v.match(/^rgba?\(([^)]+)\)$/);
  if (m) {
    // Alpha is accepted and dropped: the model has no alpha channel, and
    // silently keeping it would be a lie about what got stored.
    const p = m[1].split(/[,\s/]+/).filter(Boolean).map(Number);
    if (p.length >= 3 && p.slice(0, 3).every((n) => Number.isFinite(n))) {
      return hx(p[0]) + hx(p[1]) + hx(p[2]);
    }
    return null;
  }
  m = v.match(/^hsla?\(([^)]+)\)$/);
  if (m) {
    const p = m[1].split(/[,\s/]+/).filter(Boolean);
    const h = ((parseFloat(p[0]) % 360) + 360) % 360;
    const sat = parseFloat(p[1]) / 100;
    const li = parseFloat(p[2]) / 100;
    if (![h, sat, li].every(Number.isFinite)) return null;
    const c = (1 - Math.abs(2 * li - 1)) * sat;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const mm = li - c / 2;
    const seg = [[c,x,0],[x,c,0],[0,c,x],[0,x,c],[x,0,c],[c,0,x]][Math.floor(h / 60) % 6];
    return hx((seg[0] + mm) * 255) + hx((seg[1] + mm) * 255) + hx((seg[2] + mm) * 255);
  }
  return null;
}

export function sortTarget() {
  const s = effectiveRange();
  if (s.r0 === s.r1 && s.c0 === s.c1) {
    const b = usedBounds();
    return { r0: 0, c0: 0, r1: b.rows - 1, c1: b.cols - 1 };
  }
  return s;
}

export function looksLikeHeader(s) {
  if (s.r1 <= s.r0) return false;
  let differs = false;
  for (let c = s.c0; c <= Math.min(s.c1, s.c0 + 63); c += 1) {
    let head, below;
    try {
      head = JSON.parse(wasm.session_cells(state.sheet, s.r0, c, s.r0, c))[0];
      below = JSON.parse(wasm.session_cells(state.sheet, s.r0 + 1, c, s.r0 + 1, c))[0];
    } catch { return false; }
    const headText = head && head.t && !head.n;
    const belowNumeric = below && below.n;
    if (headText && belowNumeric) differs = true;
    // A number in the heading row is strong evidence it is data, not a heading.
    if (head && head.n) return false;
  }
  return differs;
}

export function sortRange(desc) {
  const s = sortTarget();
  const hasHeader = looksLikeHeader(s);
  const key = Math.min(Math.max(state.sel.col, s.c0), s.c1);
  applySort(s, [{ col: key, asc: !desc }], hasHeader);
}

export function applySort(s, keys, hasHeader) {
  const first = hasHeader ? s.r0 + 1 : s.r0;
  if (first >= s.r1) { status.textContent = "nothing to sort"; return; }
  try {
    wasm.session_sort_range_multi(
      state.sheet, first, s.c0, s.r1, s.c1,
      new Uint32Array(keys.map((k) => k.col)),
      new Uint8Array(keys.map((k) => (k.asc ? 1 : 0))),
    );
    const by = keys.map((k) => `${colName(k.col)} ${k.asc ? "A→Z" : "Z→A"}`).join(", then ");
    status.textContent = `sorted by ${by}${hasHeader ? " (header kept)" : ""}`;
  } catch (e) { statusError(errText(e)); }
  draw();
}

/// Columns holding data on the affected rows that the comparison does not cover.
///
/// The engine deletes whole *sheet rows*, so anything on those rows outside the
/// compared columns goes with them. Selecting A1:A6 and removing duplicates
/// deleted three values from column E — off-screen, in a column the user never
/// touched, under a status bar that said "removed 3 duplicate rows".
function dataBesideTheComparison(first, r1, c0, c1) {
  const b = usedBounds();
  const outside = [];
  for (let c = 0; c < b.cols; c += 1) {
    if (c >= c0 && c <= c1) continue;
    for (let r = first; r <= r1; r += 1) {
      let text = "";
      try { text = wasm.session_cell_input(state.sheet, r, c); } catch { text = ""; }
      if (text !== "") { outside.push(c); break; }
    }
  }
  return outside;
}

export async function removeDuplicates() {
  const s = sortTarget();
  const hasHeader = looksLikeHeader(s);
  const first = hasHeader ? s.r0 + 1 : s.r0;
  const beside = dataBesideTheComparison(first, s.r1, s.c0, s.c1);

  // Excel notices adjacent data and offers to widen before it does anything.
  // Widening is offered rather than assumed, because comparing more columns
  // finds fewer duplicates — it is a different question, not a safer version of
  // the same one. Declining cancels: with the engine deleting whole rows there
  // is no third option that keeps the narrow comparison *and* the data.
  if (beside.length) {
    const names = beside.slice(0, 4).map(colName).join(", ");
    const more = beside.length > 4 ? `, and ${beside.length - 4} more` : "";
    const widen = await confirmModal(
      "There is data next to the selection",
      `Column${beside.length === 1 ? "" : "s"} ${names}${more} also hold data on these rows, and removing a ` +
        `duplicate row removes the whole row — so those values would go too. ` +
        `Compare the whole block instead, so nothing is lost?`,
      "Compare the whole block",
    );
    canvas.focus();
    if (!widen) return;
    const b = usedBounds();
    s.c0 = 0;
    s.c1 = Math.max(0, b.cols - 1);
  }

  const ok = await confirmModal(
    "Remove duplicates",
    `Compare ${colName(s.c0)}${first + 1}:${colName(s.c1)}${s.r1 + 1} and delete rows that repeat an earlier one` +
      `${hasHeader ? ", keeping the header row" : ""}. Rows below shift up.`,
    "Remove",
  );
  canvas.focus();
  if (!ok) return;
  try {
    const n = wasm.session_remove_duplicates(state.sheet, first, s.c0, s.r1, s.c1);
    status.textContent = n ? `removed ${n} duplicate ${n === 1 ? "row" : "rows"}` : "no duplicates found";
  } catch (e) { statusError(errText(e)); }
  draw();
}

export function toggleFilter() {
  if (!wasm) return;
  if (filterInfo) {
    tryEdit(() => wasm.session_clear_filter(state.sheet));
    status.textContent = "filter removed";
  } else {
    // The engine decides what the selection means: a genuinely two-dimensional
    // one is taken as given, anything thinner grows to the block around it.
    // Selecting the row 1 header and pressing Filter is the ordinary way to
    // reach this, and taken literally it asks for one row across all 16384
    // columns — no rows beneath the header, so every checklist was empty, and a
    // button on every column of the sheet.
    const r = effectiveRange();
    let box = null;
    try {
      const j = wasm.session_filter_range_for(state.sheet, r.r0, r.c0, r.r1, r.c1);
      if (j && j !== "null") box = JSON.parse(j);
    } catch {}
    if (!box) { status.textContent = "nothing to filter"; return; }
    tryEdit(() => wasm.session_set_filter_range(state.sheet, box.r0, box.c0, box.r1, box.c1));
    status.textContent = "filter on — click a header button to filter a column";
  }
}

export function afterFilterChange() {
  // Counted across every filter, not just the sheet's own: a table carries its
  // own, and reading `filterInfo` alone reported "filter cleared" on the edit
  // that had just hidden rows.
  const n = filterHidden;
  status.textContent = n ? `filtered — ${n} row${n === 1 ? "" : "s"} hidden` : "filter cleared";
}

export function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

export function currentTable() {
  try {
    return JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col));
  } catch { return null; }
}

export function on(name, handler) {
  if (!listeners.has(name)) listeners.set(name, new Set());
  listeners.get(name).add(handler);
  return () => off(name, handler);
}

export function off(name, handler) {
  listeners.get(name)?.delete(handler);
}

export function emit(name, detail) {
  const set = listeners.get(name);
  if (!set || !set.size) return true;
  let prevented = false;
  const event = { ...detail, preventDefault: () => { prevented = true; } };
  // A `before…` event asks the host for permission; anything else tells it what
  // happened. That distinction decides what a throw means.
  const asksPermission = name.startsWith("before");
  for (const handler of [...set]) {
    try {
      if (handler(event) === false) prevented = true;
    } catch (err) {
      console.error(`[opencalc] ${name} listener threw`, err);
      if (asksPermission) {
        // A permission check that threw did not say yes. Reading a crash as
        // consent is how a host's own rule gets bypassed by a bug in it.
        prevented = true;
      } else {
        // Not a veto — the edit has already happened — but not silence either.
        // A `cellsChanged` handler that throws while writing to the host's
        // store means the change exists here and nowhere else, which is the
        // shape of the submission this project once dropped without a word.
        // The console is not where a user is looking.
        statusError(`the application's ${name} handler failed — it may not have saved this change`);
      }
    }
  }
  return !prevented;
}

export function emitStateEvents() {
  const r = selRect();
  const selection = `${state.sheet}:${r.r0},${r.c0},${r.r1},${r.c1}`;
  if (selection !== lastReported.selection) {
    lastReported.selection = selection;
    emit("selectionChanged", {
      sheet: state.sheet,
      range: { ...r },
      activeCell: { row: state.sel.row, col: state.sel.col },
    });
  }
  const calc = `${calcMode()}:${needsRecalc()}`;
  if (calc !== lastReported.calc) {
    lastReported.calc = calc;
    emit("calculationChanged", { mode: calcMode(), needsRecalculation: needsRecalc() });
  }
  let undo = "";
  try { undo = `${wasm.session_can_undo()}:${wasm.session_can_redo()}`; } catch {}
  if (undo !== lastReported.undo) {
    lastReported.undo = undo;
    emit("undoStateChanged", {
      canUndo: undo.startsWith("true"),
      canRedo: undo.endsWith("true"),
    });
  }
}

export function commandId(path, label) {
  const slug = String(label)
    .replace(/[…\u2026]/g, "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  return path ? `${path}.${slug}` : slug;
}

/// Every command id this mount currently offers.
///
/// **Rule-hidden commands are left out, and `runCommand` refuses them**, so the
/// menu and the API cannot disagree. A mode that takes `File ▸ Download` off
/// the menu and still runs it from `runCommand("file.download.csv-csv")` has
/// not taken it away at all — it has moved it somewhere the user cannot see and
/// a script still can, which is worse than leaving it visible.
///
/// The filter is `.oc-cmd-hidden`, the class `applyCommandRules()` stamps —
/// **not** the `hidden` attribute. Four controls are authored hidden and are
/// still perfectly runnable: `#tb-open` is the `<input type=file>` that
/// `File ▸ Open` clicks, `#tb-delete-sheet` is the control behind
/// `toolbar.delete-sheet` (whose *visible* route is the tab's context menu),
/// and `#tb-more` with its flyout are the narrow-window overflow. Filtering on
/// `hidden` would drop those from a standalone editor that has hidden nothing.
export function listCommands() {
  return [...qsa("[data-oc-command]")]
    .filter((n) => !n.classList.contains("oc-cmd-hidden"))
    .map((n) => n.dataset.ocCommand)
    .filter((v, i, a) => a.indexOf(v) === i)
    .sort();
}

// The menu, in a shape an operating system can draw.
//
// A desktop app should behave like a desktop app, which means the OS draws the
// menu bar — not an HTML strip inside the window. That needs the File/Edit/View
// tree in a form a native menu builder can read, and it must not be a second
// copy of it: two definitions of the same menu drift, and the one that drifts
// is always the one nobody is looking at.
//
// So this is derived from the **live DOM**, not from the `MENUS` literal the
// DOM is built from. The DOM is what `runCommand` dispatches against, and
// `applyCommandRules()` hides items in read-only mode — a model read from the
// literal would describe a menu the app does not currently have, and every
// disabled or hidden entry would be wrong. Same reasoning as the engine trace
// using a Proxy rather than 229 hand-written wrappers.
//
// Every leaf carries the id `runCommand` takes, so the native side holds ids
// and nothing else: no closures, no duplicated labels, no second dispatch path.
export function menuModel() {
  const labelOf = (node) =>
    node.querySelector(".mi-label")?.textContent?.trim() ||
    node.dataset.ocLabel ||
    node.textContent.trim();
  const isEnabled = (node) =>
    !node.disabled &&
    node.getAttribute("aria-disabled") !== "true" &&
    !node.hidden &&
    !node.classList.contains("oc-cmd-hidden");
  const panelFor = (id, cls) =>
    id ? document.querySelector(`${cls}[data-oc-for="${CSS.escape(id)}"]`) : null;

  // A native menu is a menu, so it gets the same treatment the HTML one does:
  // a command the rules **hid** is absent from it, not present and greyed.
  // `runCommand` now refuses a hidden id, so carrying one across would give the
  // operating system an entry that throws — the exact failure `TAURI-004`
  // avoided by deriving this from the live DOM instead of the `MENUS` literal.
  // Disabled is still carried: greyed-because-there-is-nothing-to-undo is a
  // state, where hidden-in-this-mode is a command that does not exist here.
  const ruleHidden = (node) => node.classList.contains("oc-cmd-hidden");

  const itemsOf = (container) => {
    const out = [];
    if (!container) return out;
    for (const node of container.children) {
      if (node.classList.contains("menu-sep")) {
        // Carried, because a native menu without separators is a wall of verbs.
        out.push({ kind: "separator" });
        continue;
      }
      if (node.tagName !== "BUTTON" || ruleHidden(node)) continue;
      const id = node.dataset.ocCommand;
      const sub = panelFor(id, ".menu-sub");
      if (sub) {
        out.push({ kind: "submenu", id, label: labelOf(node), items: itemsOf(sub) });
        continue;
      }
      out.push({
        kind: "item",
        id,
        label: labelOf(node),
        // The shortcut the menu already displays, so the native menu shows the
        // same one rather than inventing a second convention.
        accelerator: node.querySelector(".mi-key")?.textContent?.trim() || null,
        enabled: isEnabled(node),
        checked: typeof node._check === "function" ? !!node._check() : undefined,
      });
    }
    // Removing items leaves the separators that used to divide them: a leading
    // one, a trailing one, or two in a row, each of which draws a rule against
    // nothing. Collapsed here rather than in the Rust, because this is the file
    // that knows an item went. A menu with nothing hidden is untouched by this.
    const tidy = [];
    for (const entry of out) {
      if (entry.kind === "separator" &&
          (!tidy.length || tidy[tidy.length - 1].kind === "separator")) continue;
      tidy.push(entry);
    }
    while (tidy.length && tidy[tidy.length - 1].kind === "separator") tidy.pop();
    return tidy;
  };

  return [...qsa("#menubar .menu-top")]
    // The overflow "⋯" is a bar affordance, not a menu — a native bar has no
    // width limit and nothing to overflow into.
    .filter((b) => b.dataset.ocCommand && !ruleHidden(b))
    .map((b) => ({
      id: b.dataset.ocCommand,
      label: b.dataset.ocLabel || b.textContent.trim(),
      items: itemsOf(panelFor(b.dataset.ocCommand, ".menu-drop")),
    }));
}

// Hand the menu bar to the host. The nodes stay in the document — `runCommand`
// dispatches by clicking them, and a detached or disabled bar would give the
// native menu entries that throw.
export function setNativeChrome(on) {
  document.documentElement.classList.toggle("oc-chrome-native", !!on);
}

export function runCommand(id) {
  const node = qsa(`[data-oc-command="${CSS.escape(String(id))}"]`)[0];
  if (!node) {
    throw new Error(`unknown OpenCalc command "${id}" — listCommands() has the ids this build has`);
  }
  if (node.disabled || node.getAttribute("aria-disabled") === "true") {
    throw new Error(`the command "${id}" is disabled`);
  }
  // Hidden by the rules — a mode's capabilities, the host's own `hidden` list,
  // or read-only. Refused for the same reason a *disabled* one is: the rule is
  // a promise, and honouring it only in the menu would be no promise at all.
  // In an embedded editor the promise is the host's document, so this is the
  // difference between taking `File ▸ Open` away and merely hiding it.
  if (node.classList.contains("oc-cmd-hidden")) {
    throw new Error(`the command "${id}" is not available in this mode`);
  }
  node.click();
  return true;
}

export function applyCommandRules() {
  // `getCapabilities().readOnly` is already the engine's answer *or* the mode's
  // — see its definition — so this is one read, not two conventions.
  const viewer = getCapabilities().readOnly === true;
  // **These rules restore what they changed, and nothing else.**
  //
  // This used to be `node.hidden = on` and `node.disabled = dim` outright,
  // which silently asserts that every command in the editor starts visible and
  // enabled. Three do not, and running the rules revealed or enabled them:
  // `#tb-open` is the `<input type=file>` behind `File ▸ Open` (a bare "Choose
  // file" control appearing in the header), `#tb-more` and its flyout are the
  // narrow-window overflow the collapse logic owns, and `#tb-undo`/`#tb-redo`
  // are authored `disabled` because an empty history has nothing to undo.
  // Nobody noticed because the rules only ran when a host called
  // `setCommandRules` or `setReadOnly`; they run on **every boot** now, so the
  // assumption had to go.
  //
  // The class is our marker and `data-oc-was-*` is the state we displaced, so
  // "off" restores what was there rather than guessing that it was `false`.
  const hide = (node, on) => {
    if (on) {
      if (!node.classList.contains("oc-cmd-hidden")) {
        node.dataset.ocWasHidden = node.hidden ? "1" : "";
      }
      node.hidden = true;
      node.classList.add("oc-cmd-hidden");
    } else if (node.classList.contains("oc-cmd-hidden")) {
      node.hidden = node.dataset.ocWasHidden === "1";
      delete node.dataset.ocWasHidden;
      node.classList.remove("oc-cmd-hidden");
    }
  };
  const dimmed = (node, on) => {
    if (on) {
      if (!node.classList.contains("oc-cmd-disabled")) {
        node.dataset.ocWasDisabled = node.disabled ? "1" : "";
      }
      node.disabled = true;
      node.classList.add("oc-cmd-disabled");
    } else if (node.classList.contains("oc-cmd-disabled")) {
      node.disabled = node.dataset.ocWasDisabled === "1";
      delete node.dataset.ocWasDisabled;
      node.classList.remove("oc-cmd-disabled");
    }
  };

  for (const node of qsa("[data-oc-command]")) {
    // A top-level menu button is not a command in its own right — `file` is a
    // heading, not something anyone can run. Deciding it by the whitelist hid
    // every menu in read-only and left an empty bar. It is decided below, by
    // whether anything inside it survived.
    if (node.classList.contains("menu-top")) continue;
    const id = node.dataset.ocCommand;
    // Three sources, and **no name check anywhere in this loop**: the host's
    // own list, the mode's capabilities (`CAPABILITY_COMMANDS` in
    // `editor.core.js` says which id each capability governs), and read-only's
    // whitelist. A command is hidden because something declared it so, which is
    // what makes the set of commands a mode offers readable from the tables
    // rather than from the code that consumes them.
    const off =
      commandRules.hidden.includes(id) ||
      capabilityForbids(id) ||
      (viewer && !isReadOnlySafe(id));
    hide(node, off);
    dimmed(node, !off && commandRules.disabled.includes(id));
  }

  // A submenu whose every item went, and then the menu that opens it: an empty
  // popup is worse than an absent one.
  for (const sub of qsa(".menu-sub")) {
    const live = [...sub.querySelectorAll("button")].some((b) => !b.hidden);
    const opener = qs(`[data-oc-command="${sub.dataset.ocFor ?? "\u0000"}"]`);
    if (opener) hide(opener, !live);
  }
  for (const drop of qsa(".menu-drop")) {
    const live = [...drop.querySelectorAll("button")].some((b) => !b.hidden);
    const top = qs(`.menubar [data-oc-command="${drop.dataset.ocFor ?? "\u0000"}"]`);
    if (top) hide(top, !live);
  }
  // ...and a toolbar group with nothing left in it, which would otherwise be a
  // run of blank space where the controls used to be.
  //
  // Liveness is measured on **commands**, not on every button: when the toolbar
  // is narrow it collapses each group behind a trigger button, and that trigger
  // is not a command. Counting it kept every group "live" and left a row of
  // dropdowns that open onto nothing.
  const commandsIn = (root) => [...root.querySelectorAll("[data-oc-command]")];
  let anyGroup = false;
  for (const group of qsa(".tb-group")) {
    const cmds = commandsIn(group);
    const live = cmds.some((n) => !n.hidden);
    anyGroup ||= live;
    // **A group with no commands in it at all has been collapsed, not
    // emptied.** `collapseGroup` *moves* a narrow toolbar's controls into the
    // group's flyout, so on a small window every `.tb-group` is an empty box
    // that the collapse logic will refill when the window widens. Claiming one
    // here put `display: none !important` on it — which `expandGroup`'s
    // `hidden = false` cannot undo — and the group would have disappeared for
    // good on the next resize, in an editor that had hidden nothing.
    hide(group, cmds.length > 0 && !live);
  }
  // Same reason, from the other side: with the controls in the flyouts, the
  // groups are empty and `anyGroup` was false, which took the whole toolbar
  // away on a narrow window. The flyouts are where the controls are, so they
  // are where liveness is.
  for (const fly of qsa(".toolbar .tb-flyout")) {
    anyGroup ||= commandsIn(fly).some((n) => !n.hidden);
  }
  // Every control on the toolbar formats something, so in a viewer the whole
  // strip is empty. Removing it is not a policy choice the host should have to
  // make — it is the honest consequence of there being nothing in it.
  const toolbar = qs(".toolbar");
  if (toolbar) toolbar.classList.toggle("oc-cmd-hidden", !anyGroup);
  // The desktop shell builds its native menu bar from this DOM, so a rule
  // change that hides a command has to be republished or the bar keeps
  // offering it. Here rather than at the two callers for the reason the rest
  // of this function is here: it is the one place the rules are applied, and a
  // caller added later is covered without anybody remembering.
  window.__opencalcNative?.publishMenu?.();
}

export function followHyperlink(row, col) {
  let link = null;
  try { link = JSON.parse(wasm.session_hyperlink_at(state.sheet, row, col)); } catch {}
  if (!link) return false;
  if (link.location) {
    const [sheetName, cellRef] = link.location.includes("!")
      ? link.location.split("!")
      : [null, link.location];
    if (sheetName) {
      let names = [];
      try { names = JSON.parse(wasm.session_sheet_names()); } catch {}
      const i = names.findIndex((n) => n.toLowerCase() === sheetName.replace(/^'|'$/g, "").toLowerCase());
      if (i >= 0 && i !== state.sheet) switchSheet(i);
    }
    // `parseNameRange` is what the name box already uses, so a link's anchor
    // accepts exactly the references a user can type there.
    const at = parseNameRange(cellRef);
    if (at) {
      select(at.r0, at.c0);
      ensureVisible(at.r0, at.c0);
      draw();
    }
    return true;
  }
  if (link.target) {
    // `noopener` matters: without it the opened page can reach back through
    // `window.opener` and navigate this one.
    window.open(link.target, "_blank", "noopener,noreferrer");
    return true;
  }
  return false;
}

export function autoSum() {
  const r = effectiveRange();
  // A multi-cell selection means "total each of these columns below itself",
  // which is Excel's behaviour and the reason it is worth selecting a block.
  if (r.r1 > r.r0 || r.c1 > r.c0) {
    tryEdit(() => {
      for (let c = r.c0; c <= r.c1; c++) {
        wasm.session_set_cell(state.sheet, r.r1 + 1, c,
          `=SUM(${A1(r.r0, c)}:${A1(r.r1, c)})`);
      }
    });
    select(r.r1 + 1, r.c0);
    return;
  }
  const { row, col } = state.sel;
  const numeric = (rr, cc) => {
    try {
      const j = JSON.parse(wasm.session_cells(state.sheet, rr, cc, rr, cc));
      return j.length > 0 && j[0].n === 1;
    } catch { return false; }
  };
  // Excel walks past blanks to the nearest run of numbers, then takes that
  // whole run. Stopping at the first blank would make AutoSum useless one row
  // below a table, which is exactly where people press it.
  const runUp = () => {
    let r = row - 1;
    while (r >= 0 && !numeric(r, col)) r--;
    if (r < 0) return null;
    const end = r;
    while (r > 0 && numeric(r - 1, col)) r--;
    return [r, end];
  };
  const runLeft = () => {
    let c = col - 1;
    while (c >= 0 && !numeric(row, c)) c--;
    if (c < 0) return null;
    const end = c;
    while (c > 0 && numeric(row, c - 1)) c--;
    return [c, end];
  };
  const up = runUp();
  if (up) {
    tryEdit(() => wasm.session_set_cell(state.sheet, row, col,
      `=SUM(${A1(up[0], col)}:${A1(up[1], col)})`));
    return;
  }
  const left = runLeft();
  if (left) {
    tryEdit(() => wasm.session_set_cell(state.sheet, row, col,
      `=SUM(${A1(row, left[0])}:${A1(row, left[1])})`));
    return;
  }
  // Nothing to total: leave the cell alone and say so, rather than writing
  // `=SUM()` for the user to puzzle over.
  status.textContent = "nothing above or to the left to total";
}

export function clearSelection() {
  try { for (const s of allRanges()) wasm.session_clear_contents(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { statusError(errText(e)); }
  draw();
}

export function doUndo() {
  try {
    wasm.session_undo();
  } catch (e) {
    statusError(errText(e));
  }
  renderTabs();
  draw();
}

export function doRedo() {
  try {
    wasm.session_redo();
  } catch (e) {
    statusError(errText(e));
  }
  renderTabs();
  draw();
}

export function positionInline() {
  const m = mergeAt(state.sel.row, state.sel.col);
  const row = m ? m.r0 : state.sel.row;
  const col = m ? m.c0 : state.sel.col;
  const x = m ? fscreenX(m.c0) : colXAt(col) ?? fscreenX(col);
  const y = m ? fscreenY(m.r0) : rowYAt(row) ?? fscreenY(row);
  const x1 = m ? fscreenXEnd(m.c1) : x + colWAt(col);
  const y1 = m ? fscreenYEnd(m.r1) : y + rowHAt(row);
  // Clamp to the pane the anchor lives in. Two things go wrong otherwise: the
  // box is written in raw grid coordinates, so a cell scrolled under the frozen
  // band or the headers would have the editor floating over them; and a merge
  // straddling a freeze line has a pinned start and a scrolling end, which
  // cross over once the far half scrolls out and give the box a negative size.
  const f = state.freeze || { fc: 0, fr: 0, bodyX0: HW, bodyY0: HH };
  const rect = wrap.getBoundingClientRect();
  const z = state.zoom;
  const loX = col < f.fc ? HW : f.bodyX0, hiX = m && m.c1 < f.fc ? f.bodyX0 : rect.width / z;
  const loY = row < f.fr ? HH : f.bodyY0, hiY = m && m.r1 < f.fr ? f.bodyY0 : rect.height / z;
  const left = Math.max(loX, x), top = Math.max(loY, y);
  // Grid units → CSS pixels: the canvas is scaled, this element is not.
  inline.style.left = left * z + "px";
  inline.style.top = top * z + "px";
  inline.style.fontSize = 13 * z + "px";
  inline.style.width = Math.max(0, Math.min(x1, hiX) - left) * z + "px";
  const boxH = Math.max(0, Math.min(y1, hiY) - top) * z;
  // Grow past the cell for a multi-line entry (Alt+Enter), the way Excel's
  // in-cell editor does — the value is taller than the cell until it commits.
  inline.style.height = boxH + "px";
  if (inline.value.includes("\n")) {
    inline.style.height = Math.max(boxH, Math.min(inline.scrollHeight, 400)) + "px";
  }
}

export function startInline(initial, caretAtEnd = false) {
  beginEdit(inline, initial, caretAtEnd);
}

export function endInline() {
  endEdit();
}

export function cycleAnchors() {
  if (!editSurface) return false;
  const pos = editSurface.selectionStart;
  const span = refSpans.find((r) => pos >= r.s && pos <= r.e);
  if (!span) return false;
  const text = editSurface.value;
  const piece = text.slice(span.s, span.e);
  const bang = piece.lastIndexOf("!"); // keep 'Sheet'! / Sheet! as written
  const head = bang >= 0 ? piece.slice(0, bang + 1) : "";
  const parts = (bang >= 0 ? piece.slice(bang + 1) : piece).split(":");
  const first = A1_PART.exec(parts[0]);
  if (!first) return false;
  // 0 = A1, 1 = $A$1, 2 = A$1, 3 = $A1 — the order Excel steps through.
  const current = first[1] && first[3] ? 1 : first[3] ? 2 : first[1] ? 3 : 0;
  const next = (current + 1) % 4;
  const colAbs = next === 1 || next === 3 ? "$" : "";
  const rowAbs = next === 1 || next === 2 ? "$" : "";
  const rewritten = parts
    .map((p) => {
      const m = A1_PART.exec(p);
      return m ? `${colAbs}${m[2]}${rowAbs}${m[4]}` : p;
    })
    .join(":");
  editSurface.value = text.slice(0, span.s) + head + rewritten + text.slice(span.e);
  const caret = span.s + head.length + rewritten.length;
  editSurface.setSelectionRange(caret, caret);
  mirrorEdit();
  updateRefSpans();
  return true;
}

export function cancelEdit() {
  if (editHome && editHome.sheet !== state.sheet) {
    switchSheet(editHome.sheet, true);
    state.sel = { row: editHome.row, col: editHome.col };
    state.anchor = { ...state.sel };
  }
  if (editSurface) editSurface.value = editOriginal;
  endEdit();
  if (wasm) refreshFormulaBar();
}

export function updateCellMode() {
  if (!modeEl) return;
  let mode = "Ready";
  if (editSurface) mode = pointMode || formulaRefDrag ? "Point" : editMode;
  // In manual mode Excel writes "Calculate" here when an edit is waiting on
  // one, and it is the only cue that what is on screen is not what the
  // formulas say. Without it, turning calculation off looks like the sheet has
  // stopped working.
  else if (readOnly()) mode = "Read-only";
  else if (needsRecalc()) mode = "Calculate";
  if (modeEl.textContent !== mode) modeEl.textContent = mode;
}

export function updateNameBox() {
  if (activeEl() === cellRef) return;
  const r = selRect();
  const rows = r.r1 - r.r0 + 1, cols = r.c1 - r.c0 + 1;
  // The size readout belongs to *extending*, however it is being done — dragging
  // or Shift+arrow. It previously only appeared for the mouse, so a keyboard
  // selection gave no idea how big it had got.
  if ((state.dragging || formulaRefDrag || extending) && (rows > 1 || cols > 1)) {
    cellRef.value = `${rows}R x ${cols}C`;
  } else {
    cellRef.value = A1(state.sel.row, state.sel.col);
  }
}

export function inStringLiteral(before) {
  let inStr = false;
  for (let i = 0; i < before.length; i++) {
    if (before[i] === '"') {
      if (inStr && before[i + 1] === '"') { i++; continue; }
      inStr = !inStr;
    }
  }
  return inStr;
}

export function currentFnToken() {
  if (!editSurface) return null;
  const val = editSurface.value, pos = editSurface.selectionStart;
  if (!val.startsWith("=")) return null;
  const before = val.slice(0, pos);
  if (inStringLiteral(before)) return null; // don't autocomplete inside "text"
  const m = before.match(/([A-Za-z][A-Za-z0-9.]*)$/);
  if (!m) return null;
  const prev = before[before.length - m[1].length - 1];
  if (prev !== undefined && !"=+-*/^(,:&<>% ".includes(prev)) return null;
  return { start: pos - m[1].length, text: m[1] };
}

export function renderAutocomplete() {
  if (!acState) return;
  acEl.textContent = "";
  acState.matches.forEach((f, i) => {
    const row = document.createElement("div");
    row.className = "ac-item" + (i === acState.idx ? " active" : "");
    // oc-safe-html: built-in function names and signatures from the engine's
    // own catalogue, not from any document.
    // oc-safe-html: see the note above.
    row.innerHTML = `<span class="ac-name">${f.n}</span><span class="ac-sig">${f.sig.replace(/^[A-Z0-9]+/, "")}</span>`;
    row.addEventListener("mousedown", (e) => { e.preventDefault(); acState.idx = i; acceptAutocomplete(); });
    acEl.appendChild(row);
  });
  // Anchored under whichever surface is being typed in — the in-cell editor
  // sits over the canvas, the formula bar above it, so this is measured in
  // viewport coordinates (the menu is position: fixed) rather than as an offset
  // inside the grid wrapper.
  const r = (editSurface ?? inline).getBoundingClientRect();
  acEl.hidden = false;
  const width = acEl.offsetWidth;
  acEl.style.left = Math.max(4, Math.min(r.left, window.innerWidth - 4 - width)) + "px";
  acEl.style.top = r.bottom + 2 + "px";
}

export function callAtCaret() {
  if (!editSurface) return null;
  const val = editSurface.value;
  if (!val.startsWith("=")) return null;
  const upto = val.slice(0, editSurface.selectionStart);
  const stack = [];
  let i = 0;
  while (i < upto.length) {
    const c = upto[i];
    if (c === '"') { // skip a "…" literal, doubled quotes and all
      i += 1;
      while (i < upto.length) {
        if (upto[i] === '"') {
          if (upto[i + 1] === '"') { i += 2; continue; }
          i += 1;
          break;
        }
        i += 1;
      }
      continue;
    }
    if (c === "(") {
      // The word immediately before the paren, if any, names the call.
      const name = /([A-Za-z][A-Za-z0-9.]*)$/.exec(upto.slice(0, i));
      stack.push({ name: name ? name[1].toUpperCase() : null, arg: 0 });
    } else if (c === ")") {
      stack.pop();
    } else if (c === "," && stack.length) {
      stack[stack.length - 1].arg += 1;
    }
    i += 1;
  }
  for (let s = stack.length - 1; s >= 0; s -= 1) {
    if (stack[s].name) return stack[s];
  }
  return null;
}

export function acceptAutocomplete() {
  if (!acState || !editSurface) return;
  const name = acState.matches[acState.idx].n;
  const val = editSurface.value, pos = editSurface.selectionStart;
  editSurface.value = val.slice(0, acState.start) + name + "(" + val.slice(pos);
  const caret = acState.start + name.length + 1;
  editSurface.setSelectionRange(caret, caret);
  hideAutocomplete();
  editSurface.focus();
  mirrorEdit();
  updateSignatureTip();
}

export function refAcceptable() {
  if (!editSurface || !editSurface.value.startsWith("=")) return false;
  const raw = editSurface.value.slice(0, editSurface.selectionStart);
  if (inStringLiteral(raw)) return false; // caret inside a "text" literal
  const before = raw.trimEnd();
  if (before === "=") return true;
  return "=+-*/^(,:&<>% ".includes(before[before.length - 1]);
}

export function insertRef(text) {
  if (!editSurface) return;
  // Picking on another sheet writes a qualified reference. Quote the name when
  // it is not a bare word, and double any quote inside it, as Excel does.
  if (editHome && editHome.sheet !== state.sheet) {
    const name = sheetNameAt(state.sheet) || "";
    const q = /^[A-Za-z_][A-Za-z0-9_.]*$/.test(name) ? name : `'${name.replace(/'/g, "''")}'`;
    text = `${q}!${text}`;
  }
  const pending = formulaRefDrag ?? pointMode;
  const val = editSurface.value;
  const start = pending ? pending.start : editSurface.selectionStart;
  const end = pending ? pending.end : editSurface.selectionStart;
  editSurface.value = val.slice(0, start) + text + val.slice(end);
  const caret = start + text.length;
  if (pending) { pending.end = caret; editSurface.setSelectionRange(caret, caret); }
  else editSurface.setSelectionRange(caret, caret);
  mirrorEdit();
  updateRefSpans();
}

/// The Excel serial for right now, in **local** time (`CALC-VOL-01`).
///
/// Whole days since 1899-12-30, with the time of day as the fraction; 25569 is
/// 1970-01-01, which is where the epoch lands on that scale.
///
/// Local, not UTC, because that is what `TODAY` means to a person: a
/// spreadsheet that rolls over at midnight UTC shows yesterday's date to most
/// of the world for part of every day. `getTimezoneOffset` is minutes to *add*
/// to local time to reach UTC, so it is subtracted here.
export function excelSerialNow() {
  const now = new Date();
  const local = now.getTime() - now.getTimezoneOffset() * 60_000;
  return local / 86_400_000 + 25569;
}

/// Give the engine the clock and seed its volatile functions read.
///
/// The engine deliberately reads no clock — a calc engine that reaches for the
/// wall clock cannot be tested or replayed — so the host supplies one. **No
/// host ever did**, which is why `TODAY()` returned 0 and `RAND()` returned one
/// fixed sequence for as long as those functions have existed.
///
/// Called before each edit and each explicit recalculation rather than once at
/// load: that is what makes `NOW()` current and `RAND()` reroll, which is what
/// Excel does on every recalculation.
export function syncVolatileClock() {
  try { wasm.session_set_volatile(excelSerialNow(), Date.now() % 9007199254740991); }
  catch { /* an engine that has not booted yet has nothing to tell */ }
}

export function tryEdit(fn) {
  // Before the edit, because an edit recalculates: setting the clock afterwards
  // would date every volatile cell one edit behind.
  syncVolatileClock();
  try { fn(); } catch (e) { statusError(errText(e)); }
  // Any edit can add, remove or re-wrap a grown row, so the growth map — and
  // every offset derived from it — has to be rebuilt.
  invalidateGrowth();
  draw();
}

export function insertLines() {
  const r = effectiveRange();
  const rn = r.r1 - r.r0 + 1, cn = r.c1 - r.c0 + 1;
  if (state.selKind === "cols") tryEdit(() => wasm.session_insert_columns(state.sheet, r.c0, cn));
  else tryEdit(() => wasm.session_insert_rows(state.sheet, r.r0, rn));
}

export function deleteLines() {
  const r = effectiveRange();
  const rn = r.r1 - r.r0 + 1, cn = r.c1 - r.c0 + 1;
  if (state.selKind === "cols") tryEdit(() => wasm.session_delete_columns(state.sheet, r.c0, cn));
  else tryEdit(() => wasm.session_delete_rows(state.sheet, r.r0, rn));
}

export function shiftIsRisky(probe) {
  try {
    return Boolean(probe());
  } catch {
    return true;
  }
}

export function fillWithin(dir) {
  const s = effectiveRange();
  const src = dir === "down"
    ? { r0: s.r0, c0: s.c0, r1: s.r0, c1: s.c1 }
    : { r0: s.r0, c0: s.c0, r1: s.r1, c1: s.c0 };
  if ((dir === "down" && s.r1 <= s.r0) || (dir === "right" && s.c1 <= s.c0)) {
    status.textContent = "select the cells to fill";
    return;
  }
  try {
    wasm.session_fill(state.sheet, src.r0, src.c0, src.r1, src.c1, s.r0, s.c0, s.r1, s.c1);
    status.textContent = dir === "down" ? "filled down" : "filled right";
  } catch (e) { statusError(errText(e)); }
  draw();
}

export function autofillToNeighbour() {
  const s = effectiveRange();
  const probe = s.c0 > 0 ? s.c0 - 1 : s.c1 + 1;
  let end = s.r1;
  try {
    const edge = JSON.parse(wasm.session_edge(state.sheet, s.r1, probe, 1, 0));
    end = Math.max(end, edge.row);
  } catch {}
  if (end <= s.r1) { status.textContent = "nothing to fill alongside"; return; }
  try {
    wasm.session_fill(state.sheet, s.r0, s.c0, s.r1, s.c1, s.r0, s.c0, end, s.c1);
    status.textContent = `filled to row ${end + 1}`;
  } catch (e) { statusError(errText(e)); }
  draw();
}

export function seed() {
  const set = (r, c, v) => wasm.session_set_cell(0, r, c, v);
  set(0, 0, "Item"); set(0, 1, "Qty"); set(0, 2, "Price"); set(0, 3, "Total");
  set(1, 0, "Widget"); set(1, 1, "3"); set(1, 2, "4.50"); set(1, 3, "=B2*C2");
  set(2, 0, "Gadget"); set(2, 1, "5"); set(2, 2, "2"); set(2, 3, "=B3*C3");
  set(3, 0, "Gizmo"); set(3, 1, "2"); set(3, 2, "9.99"); set(3, 3, "=B4*C4");
  set(4, 0, "Total"); set(4, 3, "=SUM(D2:D4)");
  // A styled header row + a highlighted total.
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  const headerFill = dark ? "24303f" : "e8eef7";
  for (let c = 0; c < 4; c++) wasm.session_set_style(0, 0, c, true, headerFill);
  wasm.session_set_style(0, 4, 3, true, "");
  wasm.session_set_style(0, 4, 0, true, "");
  // The seeded sheet is the document, not something the user did to it.
  // Without this, holding Ctrl+Z takes the demo apart cell by cell and leaves
  // an empty grid — and Undo starts out enabled on a document nobody has
  // touched, which is its own small lie.
  wasm.session_clear_history();
  // The document the user was handed is the baseline: the seeding writes above
  // are edits as far as the engine is concerned, and without this a demo sheet
  // nobody touched would warn on close.
  markSaved();
  select(0, 0);
}

export function resetToOrigin() {
  resetView();
}

export function autofitColumnForTest(col) {
  autofitColumn(col);
}

export function autofitRowForTest(row) {
  autofitRow(row);
}

export function scrollStateForTest() {
  // **The raw inputs, not the viewport `ensureVisible` computed from them.**
  //
  // The defect this guards was a unit conversion *inside* that calculation, so
  // a hook that handed back its result would agree with the bug and assert
  // nothing. The test derives the viewport itself from the rectangle, the zoom
  // and the frozen origin — three things it can read independently — and checks
  // the scroll offset against that (`UX-GRID-02`).
  const rect = wrap.getBoundingClientRect();
  const f = state.freeze || { fc: 0, fr: 0, bodyX0: HW, bodyY0: HH };
  return {
    scrollX: state.scrollX,
    scrollY: state.scrollY,
    row: state.sel.row,
    col: state.sel.col,
    zoom: state.zoom || 1,
    rectW: rect.width,
    rectH: rect.height,
    bodyX0: f.bodyX0,
    bodyY0: f.bodyY0,
  };
}

export function selectForTest(row, col) {
  select(row, col);
}

// Shift+click without a mouse: grow the selection from wherever it is to
// (row, col), leaving the active cell — and so the toolbar's representative
// cell — where `selectForTest` put it. A test cannot otherwise build the
// multi-cell selection that the "is this selection uniform?" mirror is about.
export function extendSelectionForTest(row, col) {
  extend(row, col);
}

export function selectionRectForTest() {
  return effectiveRange();
}

/// Where a cell is on the canvas, in CSS pixels (`UX-SEL-06`).
///
/// A test that wants to know what a cell *looks like* has to read the canvas,
/// and to read the canvas it has to know where the cell is. Computing that in
/// the test would mean recomputing the layout the renderer just did — and
/// agreeing with it by construction, which is how a rendering test passes
/// against a rendering bug.
export function cellBoxForTest(row, col) {
  const x = colXAt(col);
  const y = rowYAt(row);
  if (x === undefined || y === undefined) return null;
  return { x, y, w: colWAt(col), h: rowHAt(row) };
}

export function wasmApi() {
  return wasm;
}

export function personalViewForTest() {
  const heights = JSON.parse(wasm.session_row_px(state.sheet, 0, 8));
  return {
    sheet: state.sheet,
    hasView: wasm.session_has_personal_view(state.sheet),
    // A row whose height is 0 is hidden, whichever set hid it.
    rowHeights: heights,
    visibleRows: heights.map((h, i) => (h > 0 ? i : -1)).filter((i) => i >= 0),
    // The shared half, which co-editors also see.
    sharedHidden: filterHidden,
  };
}

export function personalFilterForTest(col, values) {
  applyPersonalFilter(col, values);
  afterFilterChange();
}

export function gridHandlesForTest() {
  return {
    hiddenCols: hiddenColMarks.map((m) => ({ x: m.x, from: m.from, to: m.to })),
    hiddenRows: hiddenRowMarks.map((m) => ({ y: m.y, from: m.from, to: m.to })),
    freezeHandles: {
      col: state.freeze.fc === 0 ? { x: HW - 3, y: HH * 0.4 } : null,
      row: state.freeze.fr === 0 ? { x: HW * 0.4, y: HH - 3 } : null,
    },
    freeze: { fc: state.freeze.fc, fr: state.freeze.fr },
    // Select-all is a *kind*, not a span — it does not widen the range, so a
    // test cannot see it by measuring one.
    selKind: state.selKind,
    zoom: state.zoom,
  };
}
