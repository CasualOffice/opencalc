// OpenCalc canvas grid editor. The WASM engine owns the workbook, computes the
// layout + display text, and recalculates; this file draws the grid and text on
// a canvas and routes edits back to the engine.
import init, * as wasm from "./pkg/casual_calc_wasm.js";

const HW = 46; // row-header width (px)
const HH = 24; // column-header height (px)
let COL_W = 64;
let ROW_H = 20;

const state = {
  sheet: 0,
  scrollX: 0, // absolute content pixel offset (left of the viewport)
  scrollY: 0, // absolute content pixel offset (top of the viewport)
  firstRow: 0, // first visible row (derived from scrollY in measure())
  firstCol: 0, // first visible column (derived from scrollX in measure())
  sel: { row: 0, col: 0 }, // focus cell
  anchor: { row: 0, col: 0 }, // selection anchor
  selKind: "cells", // "cells" | "rows" | "cols" | "all"
  dragging: false,
  editing: false,
  resize: null, // active header resize: { axis:"col"|"row", index, previewPx, scope }
};

// The normalized selection rectangle (inclusive) from anchor..focus.
function selRect() {
  return {
    r0: Math.min(state.anchor.row, state.sel.row),
    c0: Math.min(state.anchor.col, state.sel.col),
    r1: Math.max(state.anchor.row, state.sel.row),
    c1: Math.max(state.anchor.col, state.sel.col),
  };
}
const DEFAULT_SCROLL_DAMP = 0.8; // rows-per-wheel factor; tunable in settings
let scrollDamp = DEFAULT_SCROLL_DAMP;

const canvas = document.getElementById("grid");
const ctx = canvas.getContext("2d");
const wrap = document.getElementById("grid-wrap");
const inline = document.getElementById("inline-edit");
const fInput = document.getElementById("formula-input");
const cellRef = document.getElementById("cell-ref");
const status = document.getElementById("tb-status");

const css = (name) => getComputedStyle(document.body).getPropertyValue(name).trim();
let colors = {};
function readColors() {
  colors = {
    bg: css("--bg") || "#fff",
    fg: css("--fg") || "#111",
    muted: css("--muted") || "#666",
    grid: css("--border") || "#e2e8f0",
    headerBg: css("--card") || "#f6f8fb",
    accent: css("--accent") || "#2f6df6",
    sel: (css("--accent") || "#2f6df6") + "22",
  };
}

function colName(n) {
  let s = "";
  n += 1;
  while (n > 0) {
    n -= 1;
    s = String.fromCharCode(65 + (n % 26)) + s;
    n = Math.floor(n / 26);
  }
  return s;
}

// Per-frame geometry of the visible window: the engine supplies each visible
// column's width and row's height (real `.xlsx` sizes), and we accumulate them
// into leading-edge offsets so drawing and hit-testing honor variable sizing.
const geo = {
  colW: [], // width (px) of the i-th visible column (firstCol + i)
  colX: [], // canvas x of its leading edge (includes the HW header)
  rowH: [],
  rowY: [],
  cols: 0, // columns whose leading edge is within the viewport
  rows: 0,
};

const MIN_LINE = 8; // conservative floor used to bound how many lines to fetch
const RESIZE_GRAB = 5; // px proximity to a header boundary that arms a resize

// Measure the wrap, (re)build `geo` from engine sizes at the current pixel
// scroll, and return `{ w, h }`. Scrolling is fluid: `scrollX/scrollY` are
// absolute content offsets, so the first visible line can be partially clipped.
function measure() {
  const rect = wrap.getBoundingClientRect();
  const v = { w: rect.width, h: rect.height };
  if (!wasm) {
    geo.colW = geo.colX = geo.rowH = geo.rowY = [];
    geo.cols = geo.rows = 0;
    return v;
  }
  // Which line sits at the viewport edge, and by how many pixels it's clipped.
  state.firstCol = wasm.session_col_at_px(state.sheet, Math.round(state.scrollX));
  state.firstRow = wasm.session_row_at_px(state.sheet, Math.round(state.scrollY));
  const subX = state.scrollX - wasm.session_col_offset_px(state.sheet, state.firstCol);
  const subY = state.scrollY - wasm.session_row_offset_px(state.sheet, state.firstRow);

  const colCap = Math.max(4, Math.ceil((v.w - HW) / MIN_LINE) + 2);
  const rowCap = Math.max(4, Math.ceil((v.h - HH) / MIN_LINE) + 2);
  geo.colW = JSON.parse(wasm.session_col_px(state.sheet, state.firstCol, colCap));
  geo.rowH = JSON.parse(wasm.session_row_px(state.sheet, state.firstRow, rowCap));

  // Live resize preview: override the affected line sizes before layout so the
  // grid reflows under the cursor without committing an edit yet. Scope covers a
  // single line, the selected band, or every line (whole-sheet selection).
  if (state.resize) {
    const rz = state.resize;
    const arr = rz.axis === "col" ? geo.colW : geo.rowH;
    const base = rz.axis === "col" ? state.firstCol : state.firstRow;
    for (let i = 0; i < arr.length; i++) {
      const idx = base + i;
      const hit = rz.scope === "all" || (rz.scope === "band" ? idx >= rz.b0 && idx <= rz.b1 : idx === rz.index);
      if (hit) arr[i] = rz.previewPx;
    }
  }

  geo.colX = new Array(geo.colW.length);
  geo.rowY = new Array(geo.rowH.length);
  let x = HW - subX;
  geo.cols = 0;
  for (let i = 0; i < geo.colW.length; i++) {
    geo.colX[i] = x;
    if (x < v.w) geo.cols = i + 1;
    x += geo.colW[i] || COL_W;
  }
  let y = HH - subY;
  geo.rows = 0;
  for (let i = 0; i < geo.rowH.length; i++) {
    geo.rowY[i] = y;
    if (y < v.h) geo.rows = i + 1;
    y += geo.rowH[i] || ROW_H;
  }
  return v;
}

// Pixel width for an OOXML border line-style token.
function borderWidth(style) {
  if (style === "thick" || style === "double") return 2;
  if (style === "medium" || style === "mediumDashed") return 1.5;
  return 1; // thin, hair, dashed, dotted, …
}

// Draw one cell-border edge from a "style:color" spec (color may be empty).
function drawEdge(spec, x0, y0, x1, y1) {
  if (!spec) return;
  const sep = spec.indexOf(":");
  const style = sep >= 0 ? spec.slice(0, sep) : spec;
  const color = sep >= 0 ? spec.slice(sep + 1) : "";
  const width = borderWidth(style);
  ctx.strokeStyle = color ? "#" + color : colors.fg;
  ctx.lineWidth = width;
  // A 1px line lands crisply on a half-pixel; wider lines centre on the edge.
  const off = width === 1 ? 0.5 : 0;
  ctx.setLineDash(style === "dashed" || style === "mediumDashed" ? [4, 2] : style === "dotted" ? [1, 2] : []);
  ctx.beginPath();
  ctx.moveTo(Math.floor(x0) + off, Math.floor(y0) + off);
  ctx.lineTo(Math.floor(x1) + off, Math.floor(y1) + off);
  ctx.stroke();
  ctx.setLineDash([]);
}

// A header boundary under the pointer, if any (for resize hit-testing).
function boundaryAt(px, py) {
  if (py < HH && px >= HW) {
    for (let i = 0; i < geo.colX.length; i++) {
      if (Math.abs(px - (geo.colX[i] + geo.colW[i])) <= RESIZE_GRAB)
        return { axis: "col", index: state.firstCol + i };
    }
  } else if (px < HW && py >= HH) {
    for (let i = 0; i < geo.rowY.length; i++) {
      if (Math.abs(py - (geo.rowY[i] + geo.rowH[i])) <= RESIZE_GRAB)
        return { axis: "row", index: state.firstRow + i };
    }
  }
  return null;
}

// Absolute column/row → visible-window index (or -1 if outside the fetched span).
const colIdx = (col) => col - state.firstCol;
const rowIdx = (row) => row - state.firstRow;
const colWAt = (col) => geo.colW[colIdx(col)] ?? COL_W;
const rowHAt = (row) => geo.rowH[rowIdx(row)] ?? ROW_H;
const colXAt = (col) => geo.colX[colIdx(col)];
const rowYAt = (row) => geo.rowY[rowIdx(row)];

// The clipped [x, x+w) pixel span covering columns c0..c1 within the grid body.
function spanX(c0, c1, v) {
  const li = colIdx(c0);
  const ri = colIdx(c1);
  const left = c0 < state.firstCol ? HW : li < geo.colX.length ? geo.colX[li] : v.w;
  const right =
    c1 < state.firstCol ? HW : ri < geo.colX.length ? geo.colX[ri] + geo.colW[ri] : v.w;
  const x = Math.max(HW, left);
  return { x, w: Math.max(0, Math.min(right, v.w) - x) };
}

function spanY(r0, r1, v) {
  const ti = rowIdx(r0);
  const bi = rowIdx(r1);
  const top = r0 < state.firstRow ? HH : ti < geo.rowY.length ? geo.rowY[ti] : v.h;
  const bot =
    r1 < state.firstRow ? HH : bi < geo.rowY.length ? geo.rowY[bi] + geo.rowH[bi] : v.h;
  const y = Math.max(HH, top);
  return { y, h: Math.max(0, Math.min(bot, v.h) - y) };
}

function resize() {
  const rect = wrap.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(rect.width * dpr);
  canvas.height = Math.floor(rect.height * dpr);
  canvas.style.width = rect.width + "px";
  canvas.style.height = rect.height + "px";
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  draw();
}

function draw() {
  const v = measure();
  ctx.clearRect(0, 0, v.w, v.h);
  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, v.w, v.h);

  const rectSel = selRect();
  // The highlighted rectangle spans the whole row/column/sheet for header/corner
  // selections, and stays clamped to the visible body otherwise.
  let sX, sY;
  if (state.selKind === "all") {
    sX = { x: HW, w: v.w - HW };
    sY = { y: HH, h: v.h - HH };
  } else if (state.selKind === "rows") {
    sX = { x: HW, w: v.w - HW };
    sY = spanY(rectSel.r0, rectSel.r1, v);
  } else if (state.selKind === "cols") {
    sX = spanX(rectSel.c0, rectSel.c1, v);
    sY = { y: HH, h: v.h - HH };
  } else {
    sX = spanX(rectSel.c0, rectSel.c1, v);
    sY = spanY(rectSel.r0, rectSel.r1, v);
  }

  // Everything in the grid body is clipped so partially-scrolled first cells
  // never bleed into the header strips (which are painted on top afterwards).
  ctx.save();
  ctx.beginPath();
  ctx.rect(HW, HH, Math.max(0, v.w - HW), Math.max(0, v.h - HH));
  ctx.clip();

  // Selection highlight (behind text).
  ctx.fillStyle = colors.sel;
  ctx.fillRect(sX.x, sY.y, sX.w, sY.h);

  // Gridlines (at each visible column/row leading edge).
  ctx.strokeStyle = colors.grid;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = 0; i <= geo.cols; i++) {
    const x = Math.floor(i < geo.colX.length ? geo.colX[i] : v.w) + 0.5;
    ctx.moveTo(x, HH);
    ctx.lineTo(x, v.h);
  }
  for (let i = 0; i <= geo.rows; i++) {
    const y = Math.floor(i < geo.rowY.length ? geo.rowY[i] : v.h) + 0.5;
    ctx.moveTo(HW, y);
    ctx.lineTo(v.w, y);
  }
  ctx.stroke();

  // Cell fills + text (from the engine).
  const lastRow = state.firstRow + geo.rows;
  const lastCol = state.firstCol + geo.cols;
  const items = JSON.parse(
    wasm.session_cells(state.sheet, state.firstRow, state.firstCol, lastRow, lastCol),
  );
  ctx.textBaseline = "middle";
  for (const it of items) {
    if (!it.bg) continue;
    const x = colXAt(it.c);
    const y = rowYAt(it.r);
    if (x === undefined || y === undefined) continue;
    ctx.fillStyle = "#" + it.bg;
    ctx.fillRect(x + 1, y + 1, colWAt(it.c) - 1, rowHAt(it.r) - 1);
  }
  for (const it of items) {
    if (!it.t) continue;
    const x = colXAt(it.c);
    const yTop = rowYAt(it.r);
    if (x === undefined || yTop === undefined) continue;
    const w = colWAt(it.c);
    const h = rowHAt(it.r);
    const y = yTop + h / 2;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, yTop, w, h);
    ctx.clip();
    const weight = it.b ? "600 " : "";
    const slant = it.i ? "italic " : "";
    ctx.font = `${slant}${weight}13px system-ui, sans-serif`;
    ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
    let tx, textAlign;
    if (it.a === "r") { textAlign = "right"; tx = x + w - 5; }
    else if (it.a === "c") { textAlign = "center"; tx = x + w / 2; }
    else { textAlign = "left"; tx = x + 5; }
    ctx.textAlign = textAlign;
    ctx.fillText(it.t, tx, y);
    if (it.u) {
      // Underline: a line just below the text baseline, spanning the glyph run.
      const tw = Math.min(ctx.measureText(it.t).width, w - 8);
      let ux = tx;
      if (textAlign === "right") ux = tx - tw;
      else if (textAlign === "center") ux = tx - tw / 2;
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(ux, y + 7.5);
      ctx.lineTo(ux + tw, y + 7.5);
      ctx.stroke();
    }
    ctx.restore();
  }

  // Cell borders (from the engine), drawn over fills/text.
  for (const it of items) {
    if (!it.bd) continue;
    const x = colXAt(it.c);
    const yTop = rowYAt(it.r);
    if (x === undefined || yTop === undefined) continue;
    const w = colWAt(it.c);
    const h = rowHAt(it.r);
    drawEdge(it.bd.l, x, yTop, x, yTop + h);
    drawEdge(it.bd.r, x + w, yTop, x + w, yTop + h);
    drawEdge(it.bd.t, x, yTop, x + w, yTop);
    drawEdge(it.bd.b, x, yTop + h, x + w, yTop + h);
  }

  // Range border (cell selections only) + focus-cell border.
  ctx.strokeStyle = colors.accent;
  ctx.lineWidth = 2;
  if (state.selKind === "cells" && sX.w > 0 && sY.h > 0) {
    ctx.strokeRect(sX.x + 1, sY.y + 1, sX.w - 1, sY.h - 1);
  }
  const fx = colXAt(state.sel.col);
  const fy = rowYAt(state.sel.row);
  if (fx !== undefined && fy !== undefined) {
    ctx.strokeRect(fx + 1, fy + 1, colWAt(state.sel.col) - 1, rowHAt(state.sel.row) - 1);
  }
  ctx.restore(); // end body clip

  // Which columns/rows are covered by the current selection (for header tint).
  const rr = selRect();
  const colInSel = (c) => state.selKind === "all" ||
    (state.selKind === "cols" ? c >= rr.c0 && c <= rr.c1 : state.selKind === "cells" && c >= rr.c0 && c <= rr.c1);
  const rowInSel = (r) => state.selKind === "all" ||
    (state.selKind === "rows" ? r >= rr.r0 && r <= rr.r1 : state.selKind === "cells" && r >= rr.r0 && r <= rr.r1);

  // Headers (painted over the body edges), with the selected band tinted.
  ctx.fillStyle = colors.headerBg;
  ctx.fillRect(0, 0, v.w, HH);
  ctx.fillRect(0, 0, HW, v.h);
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  ctx.save();
  ctx.beginPath();
  ctx.rect(HW, 0, Math.max(0, v.w - HW), HH);
  ctx.clip();
  for (let i = 0; i < geo.cols; i++) {
    const c = state.firstCol + i;
    if (colInSel(c)) { ctx.fillStyle = colors.sel; ctx.fillRect(geo.colX[i], 0, geo.colW[i], HH); }
    ctx.fillStyle = colInSel(c) ? colors.accent : colors.muted;
    ctx.fillText(colName(c), geo.colX[i] + geo.colW[i] / 2, HH / 2);
  }
  ctx.restore();
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, HH, HW, Math.max(0, v.h - HH));
  ctx.clip();
  for (let i = 0; i < geo.rows; i++) {
    const r = state.firstRow + i;
    if (rowInSel(r)) { ctx.fillStyle = colors.sel; ctx.fillRect(0, geo.rowY[i], HW, geo.rowH[i]); }
    ctx.fillStyle = rowInSel(r) ? colors.accent : colors.muted;
    ctx.fillText(String(r + 1), HW / 2, geo.rowY[i] + geo.rowH[i] / 2);
  }
  ctx.restore();

  cellRef.textContent = colName(state.sel.col) + (state.sel.row + 1);
  if (wasm) refreshFormulaBar();
}

function refreshFormulaBar() {
  if (state.editing) return;
  fInput.value = wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  document.getElementById("tb-undo").disabled = !wasm.session_can_undo();
  document.getElementById("tb-redo").disabled = !wasm.session_can_redo();
  // Reflect the focus cell's formatting on the toolbar (like a real spreadsheet).
  const fmt = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col));
  const press = (id, on) => document.getElementById(id).setAttribute("aria-pressed", on ? "true" : "false");
  press("tb-bold", fmt.b);
  press("tb-italic", fmt.i);
  press("tb-underline", fmt.u);
  for (const b of document.querySelectorAll(".tb-align")) {
    b.setAttribute("aria-pressed", b.dataset.al === fmt.al ? "true" : "false");
  }
}

function cellAt(px, py) {
  if (px < HW || py < HH) return null;
  let col = state.firstCol + Math.max(0, geo.colX.length - 1);
  for (let i = 0; i < geo.colX.length; i++) {
    if (px < geo.colX[i] + geo.colW[i]) { col = state.firstCol + i; break; }
  }
  let row = state.firstRow + Math.max(0, geo.rowY.length - 1);
  for (let i = 0; i < geo.rowY.length; i++) {
    if (py < geo.rowY[i] + geo.rowH[i]) { row = state.firstRow + i; break; }
  }
  return { row, col };
}

function select(row, col) {
  const r = Math.max(0, row);
  const c = Math.max(0, col);
  state.sel = { row: r, col: c };
  state.anchor = { row: r, col: c };
  state.selKind = "cells";
  ensureVisible();
  draw();
}

// Extend the selection to (row, col), keeping the anchor.
function extend(row, col) {
  state.sel = { row: Math.max(0, row), col: Math.max(0, col) };
  state.selKind = "cells";
  ensureVisible();
  draw();
}

function ensureVisible() {
  if (!wasm) return;
  const rect = wrap.getBoundingClientRect();
  const viewW = rect.width - HW;
  const viewH = rect.height - HH;
  const cL = wasm.session_col_offset_px(state.sheet, state.sel.col);
  const cW = JSON.parse(wasm.session_col_px(state.sheet, state.sel.col, 1))[0] || COL_W;
  const rT = wasm.session_row_offset_px(state.sheet, state.sel.row);
  const rH = JSON.parse(wasm.session_row_px(state.sheet, state.sel.row, 1))[0] || ROW_H;
  if (cL < state.scrollX) state.scrollX = cL;
  else if (cL + cW > state.scrollX + viewW) state.scrollX = cL + cW - viewW;
  if (rT < state.scrollY) state.scrollY = rT;
  else if (rT + rH > state.scrollY + viewH) state.scrollY = rT + rH - viewH;
  state.scrollX = Math.max(0, state.scrollX);
  state.scrollY = Math.max(0, state.scrollY);
}

function commit(value, advance) {
  try {
    wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, value);
    status.textContent = "ok";
  } catch (e) {
    status.textContent = `error: ${e}`;
  }
  endInline();
  if (advance) state.sel.row += 1;
  ensureVisible();
  draw();
}

function usedBounds() {
  const b = JSON.parse(wasm.session_used_bounds(state.sheet));
  return { rows: Math.max(1, b.rows), cols: Math.max(1, b.cols) };
}
// The visible column/row index at a canvas x/y (for header clicks).
function colAtX(px) {
  for (let i = 0; i < geo.colX.length; i++) if (px < geo.colX[i] + geo.colW[i]) return state.firstCol + i;
  return state.firstCol + Math.max(0, geo.colX.length - 1);
}
function rowAtY(py) {
  for (let i = 0; i < geo.rowY.length; i++) if (py < geo.rowY[i] + geo.rowH[i]) return state.firstRow + i;
  return state.firstRow + Math.max(0, geo.rowY.length - 1);
}
// Whole-sheet selection (the top-left corner box). The viewport stays put.
function selectAll() {
  state.selKind = "all";
  state.anchor = { row: state.firstRow, col: state.firstCol };
  state.sel = { row: state.firstRow, col: state.firstCol };
  endInline();
  draw();
}
// Whole-row selection; the focus stays at column 0 so the view doesn't jump.
function selectRow(r, exp) {
  state.selKind = "rows";
  if (!exp) state.anchor = { row: r, col: 0 };
  state.sel = { row: r, col: 0 };
  endInline();
  draw();
}
// Whole-column selection; the focus stays at row 0.
function selectColumn(c, exp) {
  state.selKind = "cols";
  if (!exp) state.anchor = { row: 0, col: c };
  state.sel = { row: 0, col: c };
  endInline();
  draw();
}

// The cell range a formatting/clipboard op should touch, expanded for whole
// row/column/sheet selections to the used extent along the spanning axis.
function effectiveRange() {
  const r = selRect();
  const b = usedBounds();
  if (state.selKind === "all") return { r0: 0, c0: 0, r1: b.rows - 1, c1: b.cols - 1 };
  if (state.selKind === "rows") return { r0: r.r0, c0: 0, r1: r.r1, c1: b.cols - 1 };
  if (state.selKind === "cols") return { r0: 0, c0: r.c0, r1: b.rows - 1, c1: r.c1 };
  return r;
}
// Double-click a column boundary: size the column to its widest cell.
function autofitColumn(col) {
  const b = usedBounds();
  const items = JSON.parse(wasm.session_cells(state.sheet, 0, col, b.rows - 1, col));
  let maxw = 24;
  for (const it of items) {
    if (!it.t) continue;
    const weight = it.b ? "600 " : "";
    const slant = it.i ? "italic " : "";
    ctx.font = `${slant}${weight}13px system-ui, sans-serif`;
    maxw = Math.max(maxw, ctx.measureText(it.t).width);
  }
  try { wasm.session_set_col_width(state.sheet, col, Math.ceil(maxw) + 14); } catch {}
  draw();
}

// Run a formatting op over the effective selection, then redraw.
function formatSel(fn) {
  const s = effectiveRange();
  try { fn(s); } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}
function toggleBold() { formatSel((s) => wasm.session_toggle_bold(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleItalic() { formatSel((s) => wasm.session_toggle_italic(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleUnderline() { formatSel((s) => wasm.session_toggle_underline(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function setFill(hex) { formatSel((s) => wasm.session_set_fill(state.sheet, s.r0, s.c0, s.r1, s.c1, hex)); }
function setFontColor(hex) { formatSel((s) => wasm.session_set_font_color(state.sheet, s.r0, s.c0, s.r1, s.c1, hex)); }
function setAlign(al) { formatSel((s) => wasm.session_set_align(state.sheet, s.r0, s.c0, s.r1, s.c1, al)); }
function setNumberFormat(code) { formatSel((s) => wasm.session_set_number_format(state.sheet, s.r0, s.c0, s.r1, s.c1, code)); }
function setBorder(kind) { formatSel((s) => wasm.session_set_border(state.sheet, s.r0, s.c0, s.r1, s.c1, kind)); }
function toggleBorder() { setBorder("all"); }
function clearSelection() {
  const s = effectiveRange();
  try { wasm.session_clear_range(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
  draw();
}
function doUndo() { try { wasm.session_undo(); } catch {} draw(); }
function doRedo() { try { wasm.session_redo(); } catch {} draw(); }
function download(data, name, type) {
  const blob = new Blob([data], { type });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
}
function doSave() {
  download(
    wasm.session_save(),
    "opencalc.xlsx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  );
}
// Export the active sheet as delimited text (CSV/TSV/PSV).
function doSaveDelimited(delim, ext) {
  const text = wasm.session_save_delimited(state.sheet, delim);
  download(text, "opencalc." + ext, "text/plain;charset=utf-8");
}
function saveAs(fmt) {
  try {
    if (fmt === "xlsx") doSave();
    else if (fmt === "csv") doSaveDelimited(44, "csv");
    else if (fmt === "tsv") doSaveDelimited(9, "tsv");
    else if (fmt === "psv") doSaveDelimited(124, "psv");
    status.textContent = "downloaded ." + fmt;
  } catch (e) { status.textContent = `error: ${e}`; }
}
async function doCopy() {
  const s = effectiveRange();
  const tsv = wasm.session_copy_tsv(state.sheet, s.r0, s.c0, s.r1, s.c1);
  try { await navigator.clipboard.writeText(tsv); status.textContent = "copied"; }
  catch { status.textContent = "copy blocked"; }
}
async function doPaste() {
  try {
    const tsv = await navigator.clipboard.readText();
    wasm.session_paste_tsv(state.sheet, state.sel.row, state.sel.col, tsv);
    draw();
  } catch { status.textContent = "paste blocked"; }
}

function startInline(initial) {
  state.editing = true;
  const x = colXAt(state.sel.col) ?? HW;
  const y = rowYAt(state.sel.row) ?? HH;
  inline.style.display = "block";
  inline.style.left = x + "px";
  inline.style.top = y + "px";
  inline.style.width = colWAt(state.sel.col) + "px";
  inline.style.height = rowHAt(state.sel.row) + "px";
  inline.value =
    initial !== undefined
      ? initial
      : wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  inline.focus();
  if (initial === undefined) inline.select();
}

function endInline() {
  state.editing = false;
  inline.style.display = "none";
  canvas.focus();
}

const tabsEl = document.getElementById("sheet-tabs");

// Reset the viewport + selection to the top-left (e.g. on a sheet switch).
function resetView() {
  state.scrollX = state.scrollY = 0;
  state.sel = { row: 0, col: 0 };
  state.anchor = { row: 0, col: 0 };
  endInline();
  draw();
}

function switchSheet(i) {
  if (i === state.sheet) return;
  state.sheet = i;
  resetView();
  renderTabs();
}

// (Re)build the bottom sheet-tab bar from the engine's sheet list.
function renderTabs() {
  const names = JSON.parse(wasm.session_sheet_names());
  if (state.sheet >= names.length) state.sheet = names.length - 1;
  tabsEl.textContent = "";
  names.forEach((name, i) => {
    const b = document.createElement("button");
    b.className = "sheet-tab" + (i === state.sheet ? " active" : "");
    b.textContent = name;
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", i === state.sheet ? "true" : "false");
    b.addEventListener("click", () => switchSheet(i));
    b.addEventListener("dblclick", () => renameSheet(i, b));
    b.addEventListener("contextmenu", (e) => { e.preventDefault(); sheetMenu(i, e.clientX, e.clientY); });
    tabsEl.appendChild(b);
  });
  const add = document.createElement("button");
  add.className = "sheet-add";
  add.title = "Add sheet";
  add.setAttribute("aria-label", "Add sheet");
  add.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="icon-sm"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>';
  add.addEventListener("click", () => {
    try {
      const i = wasm.session_add_sheet();
      switchSheet(i);
      renderTabs();
    } catch (e) { status.textContent = `error: ${e}`; }
  });
  tabsEl.appendChild(add);
}

// Inline-rename a sheet tab.
function renameSheet(i, tabEl) {
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
    catch (e) { status.textContent = `error: ${e}`; }
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

function closeSheetMenu() {
  const m = document.getElementById("sheet-ctx");
  if (m) m.remove();
}
// Right-click context menu for a sheet tab.
function sheetMenu(i, x, y) {
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu";
  menu.id = "sheet-ctx";
  const item = (label, danger, fn) => {
    const btn = document.createElement("button");
    btn.textContent = label;
    if (danger) btn.className = "danger";
    btn.addEventListener("click", () => { closeSheetMenu(); fn(); });
    menu.appendChild(btn);
  };
  item("Rename", false, () => renameSheet(i, tabsEl.querySelectorAll(".sheet-tab")[i]));
  item("Duplicate", false, () => {
    try { const n = wasm.session_duplicate_sheet(i); switchSheet(n); renderTabs(); }
    catch (e) { status.textContent = `error: ${e}`; }
  });
  item("Delete", true, () => {
    try {
      wasm.session_delete_sheet(i);
      if (i <= state.sheet) state.sheet = Math.max(0, state.sheet - 1);
      renderTabs();
      resetView();
    } catch (e) { status.textContent = `error: ${e}`; }
  });
  menu.style.left = x + "px";
  menu.style.top = "0px";
  menu.style.visibility = "hidden";
  document.body.appendChild(menu);
  // Open upward if it would overflow the bottom (tabs sit at the viewport edge).
  const h = menu.offsetHeight;
  const top = y + h > window.innerHeight ? Math.max(4, y - h) : y;
  menu.style.top = top + "px";
  menu.style.visibility = "visible";
  setTimeout(() => document.addEventListener("click", closeSheetMenu, { once: true }), 0);
}

// Live-update the previewed size of the line being dragged.
function updateResize(px, py) {
  if (state.resize.axis === "col") {
    const left = wasm.session_col_offset_px(state.sheet, state.resize.index) - state.scrollX + HW;
    state.resize.previewPx = Math.max(MIN_LINE, Math.round(px - left));
  } else {
    const top = wasm.session_row_offset_px(state.sheet, state.resize.index) - state.scrollY + HH;
    state.resize.previewPx = Math.max(MIN_LINE, Math.round(py - top));
  }
  draw();
}

function wireEvents() {
  canvas.addEventListener("mousedown", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    // A header boundary starts a column/row resize instead of a selection.
    const hb = boundaryAt(px, py);
    if (hb) {
      endInline();
      const cur = hb.axis === "col"
        ? JSON.parse(wasm.session_col_px(state.sheet, hb.index, 1))[0] || COL_W
        : JSON.parse(wasm.session_row_px(state.sheet, hb.index, 1))[0] || ROW_H;
      // Scope: whole sheet, the selected band (if the line is in it), or just one.
      const r = selRect();
      let scope = "one", b0 = hb.index, b1 = hb.index;
      if (state.selKind === "all") scope = "all";
      else if (hb.axis === "col" && state.selKind === "cols" && hb.index >= r.c0 && hb.index <= r.c1) { scope = "band"; b0 = r.c0; b1 = r.c1; }
      else if (hb.axis === "row" && state.selKind === "rows" && hb.index >= r.r0 && hb.index <= r.r1) { scope = "band"; b0 = r.r0; b1 = r.r1; }
      state.resize = { axis: hb.axis, index: hb.index, previewPx: cur, scope, b0, b1 };
      return;
    }
    // Header clicks: select-all (corner), whole column, or whole row.
    if (px < HW && py < HH) { selectAll(); canvas.focus(); return; }
    if (py < HH && px >= HW) { selectColumn(colAtX(px)); canvas.focus(); return; }
    if (px < HW && py >= HH) { selectRow(rowAtY(py)); canvas.focus(); return; }
    const hit = cellAt(px, py);
    if (hit) {
      endInline();
      if (e.shiftKey) extend(hit.row, hit.col);
      else { select(hit.row, hit.col); state.dragging = true; }
      canvas.focus();
    }
  });
  canvas.addEventListener("mousemove", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    if (state.resize) { updateResize(px, py); return; }
    if (state.dragging) {
      const hit = cellAt(px, py);
      if (hit && (hit.row !== state.sel.row || hit.col !== state.sel.col)) extend(hit.row, hit.col);
      return;
    }
    // Idle hover: show the resize cursor when over a header boundary.
    const hb = boundaryAt(px, py);
    canvas.style.cursor = hb ? (hb.axis === "col" ? "col-resize" : "row-resize") : "cell";
  });
  window.addEventListener("mouseup", () => {
    if (state.resize) {
      const r = state.resize;
      state.resize = null;
      const px = r.previewPx;
      try {
        if (r.axis === "col") {
          if (r.scope === "all") wasm.session_set_all_col_width(state.sheet, px);
          else if (r.scope === "band") wasm.session_set_col_width_range(state.sheet, r.b0, r.b1, px);
          else wasm.session_set_col_width(state.sheet, r.index, px);
        } else {
          if (r.scope === "all") wasm.session_set_all_row_height(state.sheet, px);
          else if (r.scope === "band") wasm.session_set_row_height_range(state.sheet, r.b0, r.b1, px);
          else wasm.session_set_row_height(state.sheet, r.index, px);
        }
        status.textContent = r.scope === "one" ? "resized" : "resized all";
      } catch (e) { status.textContent = `error: ${e}`; }
      draw();
    }
    state.dragging = false;
  });
  canvas.addEventListener("dblclick", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    // Double-clicking a column boundary auto-fits it to its widest cell; a row
    // boundary resets the row to the default height.
    const hb = boundaryAt(px, py);
    if (hb) {
      if (hb.axis === "col") autofitColumn(hb.index);
      else { try { wasm.session_clear_row_height(state.sheet, hb.index); } catch {} draw(); }
      return;
    }
    const hit = cellAt(px, py);
    if (hit) {
      select(hit.row, hit.col);
      startInline();
    }
  });
  wrap.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      // Fluid pixel scrolling: move the absolute content offset directly, so the
      // grid glides smoothly instead of snapping a whole row/column at a time.
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? wrap.clientHeight : 1;
      state.scrollY = Math.max(0, state.scrollY + e.deltaY * unit * scrollDamp);
      state.scrollX = Math.max(0, state.scrollX + e.deltaX * unit * scrollDamp);
      draw();
    },
    { passive: false },
  );
  canvas.addEventListener("keydown", async (e) => {
    if (state.editing) return;
    const mod = e.ctrlKey || e.metaKey;

    // Keyboard shortcuts.
    if (mod) {
      const k = e.key.toLowerCase();
      if (k === "b") { toggleBold(); e.preventDefault(); return; }
      if (k === "i") { toggleItalic(); e.preventDefault(); return; }
      if (k === "u") { toggleUnderline(); e.preventDefault(); return; }
      if (e.shiftKey && (k === "7" || k === "&")) { toggleBorder(); e.preventDefault(); return; }
      if (e.shiftKey && (k === "l" || k === "e" || k === "r")) {
        setAlign(k === "l" ? "left" : k === "e" ? "center" : "right"); e.preventDefault(); return;
      }
      if (k === "a") { selectAll(); e.preventDefault(); return; }
      if (k === "z") { doUndo(); e.preventDefault(); return; }
      if (k === "y" || (k === "z" && e.shiftKey)) { doRedo(); e.preventDefault(); return; }
      if (k === "s") { doSave(); e.preventDefault(); return; }
      if (k === "c") { await doCopy(); e.preventDefault(); return; }
      if (k === "v") { await doPaste(); e.preventDefault(); return; }
    }

    const move = (dr, dc) => {
      if (e.shiftKey) extend(state.sel.row + dr, state.sel.col + dc);
      else select(state.sel.row + dr, state.sel.col + dc);
    };
    switch (e.key) {
      case "ArrowUp": move(-1, 0); e.preventDefault(); break;
      case "ArrowDown": move(1, 0); e.preventDefault(); break;
      case "Enter": select(state.sel.row + 1, state.sel.col); e.preventDefault(); break;
      case "ArrowLeft": move(0, -1); e.preventDefault(); break;
      case "ArrowRight": move(0, 1); e.preventDefault(); break;
      case "Tab": select(state.sel.row, state.sel.col + (e.shiftKey ? -1 : 1)); e.preventDefault(); break;
      case "Backspace": case "Delete": clearSelection(); e.preventDefault(); break;
      case "F2": startInline(); e.preventDefault(); break;
      default:
        if (e.key.length === 1 && !mod) { startInline(e.key); e.preventDefault(); }
    }
  });
  inline.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { commit(inline.value, true); canvas.focus(); e.preventDefault(); }
    else if (e.key === "Escape") { endInline(); e.preventDefault(); }
    else if (e.key === "Tab") { commit(inline.value, false); select(state.sel.row, state.sel.col + 1); e.preventDefault(); }
  });
  fInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { commit(fInput.value, false); canvas.focus(); e.preventDefault(); }
  });

  document.getElementById("tb-new").addEventListener("click", () => { wasm.session_new(); state.sheet = 0; seed(); renderTabs(); });

  // Popover menus: click toggles, outside-click / Escape closes, only one open.
  const menus = [];
  function wirePopup(btnId, menuId, onItem) {
    const btn = document.getElementById(btnId);
    const menu = document.getElementById(menuId);
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      menu.hidden = !open;
    });
    for (const item of menu.querySelectorAll("button")) {
      item.addEventListener("click", () => { onItem(item); menu.hidden = true; canvas.focus(); });
    }
  }
  document.addEventListener("click", () => { for (const m of menus) m.hidden = true; });

  wirePopup("tb-save", "save-menu", (b) => saveAs(b.dataset.fmt));
  wirePopup("tb-fontcolor", "fontcolor-menu", (b) => setFontColor(b.dataset.c));
  wirePopup("tb-fillcolor", "fillcolor-menu", (b) => setFill(b.dataset.c));
  wirePopup("tb-numfmt", "numfmt-menu", (b) => setNumberFormat(b.dataset.nf));
  wirePopup("tb-border", "border-menu", (b) => setBorder(b.dataset.bd));

  document.getElementById("tb-bold").addEventListener("click", () => { toggleBold(); canvas.focus(); });
  document.getElementById("tb-italic").addEventListener("click", () => { toggleItalic(); canvas.focus(); });
  document.getElementById("tb-underline").addEventListener("click", () => { toggleUnderline(); canvas.focus(); });
  document.getElementById("tb-currency").addEventListener("click", () => { setNumberFormat("$#,##0.00"); canvas.focus(); });
  document.getElementById("tb-percent").addEventListener("click", () => { setNumberFormat("0%"); canvas.focus(); });
  for (const b of document.querySelectorAll(".tb-align")) {
    b.addEventListener("click", () => { setAlign(b.dataset.al); canvas.focus(); });
  }
  document.getElementById("tb-open").addEventListener("change", async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const ext = (file.name.split(".").pop() || "").toLowerCase();
    // Delimiter byte by extension: tab=9, pipe=124, comma=44 (null → .xlsx).
    const delim = ext === "tsv" || ext === "tab" ? 9 : ext === "psv" ? 124 : ext === "csv" ? 44 : null;
    try {
      if (delim !== null) wasm.session_open_delimited(bytes, delim);
      else wasm.session_open(bytes);
      status.textContent = "opened " + file.name;
    } catch (err) { status.textContent = `error: ${err}`; }
    e.target.value = ""; // allow re-opening the same file
    state.sheet = 0;
    state.scrollX = state.scrollY = 0;
    renderTabs();
    select(0, 0);
  });
  document.getElementById("tb-undo").addEventListener("click", doUndo);
  document.getElementById("tb-redo").addEventListener("click", doRedo);

  window.addEventListener("resize", resize);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { readColors(); draw(); });

  wireSettings();
}

function applyTheme(theme) {
  if (theme === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  localStorage.setItem("oc-theme", theme);
  readColors();
  draw();
}

function applyAccent(color) {
  document.documentElement.style.setProperty("--accent", color);
  localStorage.setItem("oc-accent", color);
  for (const b of document.querySelectorAll("#set-accent button")) {
    b.setAttribute("aria-current", b.dataset.c === color ? "true" : "false");
  }
  readColors();
  draw();
}

function wireSettings() {
  const gear = document.getElementById("tb-settings");
  const panel = document.getElementById("settings-panel");
  const themeSel = document.getElementById("set-theme");

  gear.addEventListener("click", (e) => {
    e.stopPropagation();
    panel.hidden = !panel.hidden;
  });
  document.addEventListener("click", (e) => {
    if (!panel.contains(e.target) && e.target !== gear) panel.hidden = true;
  });
  themeSel.addEventListener("change", () => applyTheme(themeSel.value));
  for (const b of document.querySelectorAll("#set-accent button")) {
    b.addEventListener("click", () => applyAccent(b.dataset.c));
  }

  const scroll = document.getElementById("set-scroll");
  const scrollVal = document.getElementById("set-scroll-val");
  const setScroll = (v, persist) => {
    scrollDamp = v;
    scroll.value = String(v);
    scrollVal.textContent = v.toFixed(2);
    if (persist) localStorage.setItem("oc-scroll", String(v));
  };
  scroll.addEventListener("input", () => setScroll(parseFloat(scroll.value), true));

  // Restore saved preferences (default scroll speed is 0.80).
  const theme = localStorage.getItem("oc-theme") || "auto";
  themeSel.value = theme;
  applyTheme(theme);
  const accent = localStorage.getItem("oc-accent");
  if (accent) applyAccent(accent);
  const savedScroll = parseFloat(localStorage.getItem("oc-scroll"));
  setScroll(Number.isFinite(savedScroll) ? savedScroll : DEFAULT_SCROLL_DAMP, false);
}

// Seed a small demo workbook with formulas.
function seed() {
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
  select(0, 0);
}

async function main() {
  await init();
  COL_W = wasm.default_col_px();
  ROW_H = wasm.default_row_px();
  readColors();
  wasm.session_new();
  wireEvents();
  seed();
  renderTabs();
  resize();
  status.textContent = `engine v${wasm.version()}`;
}

main().catch((err) => { status.textContent = `failed: ${err}`; });
