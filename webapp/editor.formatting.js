// Toolbar formatting commands.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  SIZE_LADDER,
  allRanges,
  borderColor,
  borderStyle,
  byId,
  canvas,
  cellAt,
  closeSheetMenu,
  colWAt,
  colXAt,
  confirmModal,
  ctx,
  currentTable,
  draw,
  effectiveRange,
  el,
  errText,
  lastFill,
  ocOverlayHost,
  painter,
  positionMenu,
  refreshTablePanel,
  rowHAt,
  rowYAt,
  select,
  setPainter,
  sheetMerges,
  state,
  status,
  statusError,
  t,
  tryEdit,
  wasm,
} from "./editor.core.js";

export function mergeInSel(m) {
  const s = effectiveRange();
  return !(m.r1 < s.r0 || m.r0 > s.r1 || m.c1 < s.c0 || m.c0 > s.c1);
}

export function formatSel(fn) {
  try { for (const s of allRanges()) fn(s); } catch (e) { statusError(errText(e)); }
  draw();
}

export function toggleBold() { formatSel((s) => wasm.session_toggle_bold(state.sheet, s.r0, s.c0, s.r1, s.c1)); }

export function toggleItalic() { formatSel((s) => wasm.session_toggle_italic(state.sheet, s.r0, s.c0, s.r1, s.c1)); }

export function toggleUnderline() { formatSel((s) => wasm.session_toggle_underline(state.sheet, s.r0, s.c0, s.r1, s.c1)); }

export function toggleStrike() { formatSel((s) => wasm.session_toggle_strike(state.sheet, s.r0, s.c0, s.r1, s.c1)); }

export function setFill(hex, link) {
  formatSel((s) => wasm.session_set_fill(
    state.sheet, s.r0, s.c0, s.r1, s.c1, hex, link ? link.slot : -1, link ? link.tint : 0));
}

export function setFontColor(hex, link) {
  formatSel((s) => wasm.session_set_font_color(
    state.sheet, s.r0, s.c0, s.r1, s.c1, hex, link ? link.slot : -1, link ? link.tint : 0));
}

export function setAlign(al) { formatSel((s) => wasm.session_set_align(state.sheet, s.r0, s.c0, s.r1, s.c1, al)); }

export function setValign(va) { formatSel((s) => wasm.session_set_valign(state.sheet, s.r0, s.c0, s.r1, s.c1, va)); }

export function toggleWrap() { formatSel((s) => wasm.session_toggle_wrap(state.sheet, s.r0, s.c0, s.r1, s.c1)); }

export function setVertAlign(which) { formatSel((s) => wasm.session_toggle_vert_align(state.sheet, s.r0, s.c0, s.r1, s.c1, which)); }

export function armPainter(sticky) {
  setPainter({ row: state.sel.row, col: state.sel.col, sticky });
  status.textContent = sticky
    ? "format painter: select cells to paint (Esc to stop)"
    : "format painter: select cells to paint";
}

export function applyPainter(s) {
  if (!painter) return false;
  const { row, col, sticky } = painter;
  try {
    wasm.session_copy_style(state.sheet, row, col, s.r0, s.c0, s.r1, s.c1);
    status.textContent = "format applied";
  } catch (e) { statusError(errText(e)); }
  if (!sticky) setPainter(null);
  draw();
  return true;
}

export function setRotation(rot) {
  formatSel((s) => wasm.session_set_rotation(state.sheet, s.r0, s.c0, s.r1, s.c1, rot));
}

export function setIndent(delta) {
  formatSel((s) => wasm.session_adjust_indent(state.sheet, s.r0, s.c0, s.r1, s.c1, delta));
}

export function setTextOverflow(mode) {
  formatSel((s) => wasm.session_set_text_overflow(state.sheet, s.r0, s.c0, s.r1, s.c1, mode));
}

export function applyTableStyle(change) {
  const t = currentTable();
  if (!t) return;
  const v = (key) => (change[key] !== undefined ? change[key] : t[key]);
  // Bitmask: 1 banded rows, 2 banded columns, 4 first column, 8 last.
  const flags = (v("stripes") ? 1 : 0) | (v("colStripes") ? 2 : 0)
    | (v("firstCol") ? 4 : 0) | (v("lastCol") ? 8 : 0);
  tryEdit(() => wasm.session_set_table_style(
    state.sheet, t.r0, t.c0, v("style"), flags));
  refreshTablePanel();
}

export async function mergeVariant(kind) {
  const s = effectiveRange();
  if (kind === "none") {
    try { wasm.session_unmerge_cells(state.sheet, s.r0, s.c0, s.r1, s.c1); }
    catch (e) { statusError(errText(e)); }
    draw();
    return;
  }
  if (kind === "across") {
    if (s.c1 <= s.c0) { status.textContent = "select more than one column"; return; }
    // Count what every row would bury and ask once, rather than per row or —
    // worse — not at all: merging across is still merging, and UX-M01's rule is
    // that data is never discarded without saying so.
    let hidden = 0;
    for (let r = s.r0; r <= s.r1; r += 1) {
      try { hidden += wasm.session_merge_hidden_count(state.sheet, r, s.c0, r, s.c1); } catch {}
    }
    if (hidden > 0) {
      const ok = await confirmModal(
        "Merge across",
        `Each row keeps only its leftmost value. ${hidden} other ${hidden === 1 ? "value" : "values"} will be discarded.`,
        "Merge across",
      );
      canvas.focus();
      if (!ok) return;
    }
    for (let r = s.r0; r <= s.r1; r += 1) {
      try {
        if (hidden > 0) wasm.session_merge_cells_discarding(state.sheet, r, s.c0, r, s.c1);
        else wasm.session_merge_cells(state.sheet, r, s.c0, r, s.c1);
      } catch (e) { statusError(errText(e)); }
    }
    status.textContent = `merged ${s.r1 - s.r0 + 1} rows across`;
    draw();
    return;
  }
  await toggleMerge();
  if (kind === "center") setAlign("center");
}

export async function toggleMerge() {
  const s = effectiveRange();
  // Merging keeps only the top-left value. The engine can discard the rest in
  // the same undo step, but silently throwing away data is exactly what this
  // project says it will not do — so ask first, as Excel does.
  let hidden = 0;
  try { hidden = wasm.session_merge_hidden_count(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
  const alreadyMerged = sheetMerges.some(
    (m) => m.r0 >= s.r0 && m.c0 >= s.c0 && m.r1 <= s.r1 && m.c1 <= s.c1,
  );
  if (!alreadyMerged && hidden > 0) {
    const ok = await confirmModal(
      "Merge cells",
      `Merging keeps only the top-left value. ${hidden} other ${hidden === 1 ? "value" : "values"} in the selection will be discarded.`,
      "Merge",
    );
    canvas.focus();
    if (!ok) return;
    try { wasm.session_merge_cells_discarding(state.sheet, s.r0, s.c0, s.r1, s.c1); }
    catch (e) { statusError(errText(e)); }
    draw();
    return;
  }
  try {
    // Any selection that *contains* a merge unmerges it (Excel's rule) — the
    // exact-match test used here meant selecting a block around a merge silently
    // re-merged the block instead. The engine already drops every intersecting
    // merge, so the selection is passed through as-is.
    const m = sheetMerges.find((mm) => mm.r0 >= s.r0 && mm.c0 >= s.c0 && mm.r1 <= s.r1 && mm.c1 <= s.c1);
    if (m) {
      wasm.session_unmerge_cells(state.sheet, s.r0, s.c0, s.r1, s.c1);
    } else {
      wasm.session_merge_cells(state.sheet, s.r0, s.c0, s.r1, s.c1);
    }
  } catch (e) { statusError(errText(e)); }
  draw();
}

export function setFontName(name) { formatSel((s) => wasm.session_set_font_name(state.sheet, s.r0, s.c0, s.r1, s.c1, name)); }

export function setFontSize(pts) { formatSel((s) => wasm.session_set_font_size(state.sheet, s.r0, s.c0, s.r1, s.c1, pts)); }

export function stepFontSize(dir) {
  let cur = 11;
  try {
    const f = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col));
    if (f.fs) cur = f.fs;
  } catch {}
  const next = dir > 0
    ? (SIZE_LADDER.find((s) => s > cur) ?? Math.min(409, Math.round(cur) + 2))
    : ([...SIZE_LADDER].reverse().find((s) => s < cur) ?? Math.max(1, Math.round(cur) - 2));
  setFontSize(next);
}

export function setNumberFormat(code) { formatSel((s) => wasm.session_set_number_format(state.sheet, s.r0, s.c0, s.r1, s.c1, code)); }

export function adjustDecimals(delta) { formatSel((s) => wasm.session_adjust_decimals(state.sheet, s.r0, s.c0, s.r1, s.c1, delta)); }

export function setBorder(kind) {
  // The composite bottoms are defined by their weight, so they carry their own
  // style rather than whatever the picker happens to be set to.
  const style = kind === "bottomdouble" ? "double"
    : kind === "bottomthick" ? "thick"
    : borderStyle;
  formatSel((s) => wasm.session_set_border(state.sheet, s.r0, s.c0, s.r1, s.c1, kind, style, borderColor));
}

export function toggleBorder() { setBorder("all"); }

export function bdIcon(kind) {
  const seg = {
    top: "M3 3H17", bottom: "M3 17H17", left: "M3 3V17", right: "M17 3V17",
    midH: "M3 10H17", midV: "M10 3V17",
    diagDown: "M3 3L17 17", diagUp: "M3 17L17 3",
  };
  const bold = {
    all: ["top", "bottom", "left", "right", "midH", "midV"],
    outer: ["top", "bottom", "left", "right"],
    inner: ["midH", "midV"], horizontal: ["midH"], vertical: ["midV"],
    top: ["top"], bottom: ["bottom"], left: ["left"], right: ["right"], none: [],
    topandbottom: ["top", "bottom"], bottomdouble: ["bottom"], bottomthick: ["bottom"],
    diagdown: ["diagDown"], diagup: ["diagUp"], diagboth: ["diagDown", "diagUp"], nodiag: [],
  }[kind] || [];
  let faint = "";
  for (const k in seg) faint += `<path d="${seg[k]}" stroke="var(--oc-border-color)" stroke-width="1"/>`;
  let strong = "";
  for (const k of bold) strong += `<path d="${seg[k]}" stroke="currentColor" stroke-width="2"/>`;
  const clear = kind === "none" || kind === "nodiag"
    ? `<path d="M5 15L15 5" stroke="#e5484d" stroke-width="1.6"/>` : "";
  // The composite bottoms differ only in the weight of that one edge, so the
  // icon has to say which: a second line for double, a heavier one for thick.
  if (kind === "bottomdouble") strong += `<path d="M3 14H17" stroke="currentColor" stroke-width="2"/>`;
  if (kind === "bottomthick") strong += `<path d="M3 16.5H17" stroke="currentColor" stroke-width="3"/>`;
  return `<svg viewBox="0 0 20 20" fill="none" class="icon">${faint}${strong}${clear}</svg>`;
}

export function clearFormats() {
  try { for (const s of allRanges()) wasm.session_clear_formats(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { statusError(errText(e)); }
  draw();
}

export function hideFillOptions() {
  const b = byId("fill-options");
  if (b) b.remove();
}

export function showFillOptions(dst) {
  hideFillOptions();
  if (!lastFill) return;
  const x = colXAt(dst.c1), y = rowYAt(dst.r1);
  if (x === undefined || y === undefined) return;
  const rect = canvas.getBoundingClientRect();
  const btn = document.createElement("button");
  btn.id = "fill-options";
  btn.className = "fill-options";
  btn.title = "Fill options";
  btn.setAttribute("aria-label", "Fill options");
  btn.textContent = "⊞";
  btn.style.left = rect.left + (x + colWAt(dst.c1)) * state.zoom + "px";
  btn.style.top = rect.top + (y + rowHAt(dst.r1)) * state.zoom + "px";
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    const menu = document.createElement("div");
    menu.className = "popmenu ctx-menu";
    menu.id = "sheet-ctx";
    for (const [label, mode] of [
      ["Copy cells", "copy"],
      ["Fill series", "series"],
      ["Growth series", "growth"],
      ["Fill formatting only", "formats"],
      ["Fill without formatting", "values"],
    ]) {
      const b = el("button", "menu-item", label);
      b.addEventListener("click", () => {
        closeSheetMenu();
        const { src, dst: d } = lastFill;
        tryEdit(() => wasm.session_fill_mode(
          state.sheet, src.r0, src.c0, src.r1, src.c1, d.r0, d.c0, d.r1, d.c1, mode));
        status.textContent = label.toLowerCase();
      });
      menu.appendChild(b);
    }
    const r = btn.getBoundingClientRect();
    positionMenu(menu, r.left, r.bottom + 2);
  });
  ocOverlayHost.appendChild(btn);
}

export function updateFill(px, py) {
  const hit = cellAt(px, py);
  if (!hit) return;
  const s = state.fill.src;
  const vRows = hit.row > s.r1 ? hit.row - s.r1 : hit.row < s.r0 ? s.r0 - hit.row : 0;
  const hCols = hit.col > s.c1 ? hit.col - s.c1 : hit.col < s.c0 ? s.c0 - hit.col : 0;
  state.fill.dst = vRows >= hCols
    ? { r0: Math.min(s.r0, hit.row), c0: s.c0, r1: Math.max(s.r1, hit.row), c1: s.c1 }
    : { r0: s.r0, c0: Math.min(s.c0, hit.col), r1: s.r1, c1: Math.max(s.c1, hit.col) };
  draw();
}
