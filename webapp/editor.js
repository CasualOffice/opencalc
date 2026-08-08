// OpenCalc canvas grid editor. The WASM engine owns the workbook, computes the
// layout + display text, and recalculates; this file draws the grid and text on
// a canvas and routes edits back to the engine.
// The glue + wasm binary are loaded in main() with a build tag on the URL so a
// rebuilt engine is never shadowed by a stale browser cache. Bump BUILD (or let
// the dev server send no-store) to force a fresh fetch.
const BUILD = "32";
let init, wasm;

// Header strip sizes. Zero when the sheet hides its headers (OOXML's
// showRowColHeaders="0"), which is what makes the grid start at the very
// top-left corner: everything else measures the body as "past HW/HH", so the
// whole layout follows from these two numbers.
const HEADER_W = 46; // row-header width (px)
const HEADER_H = 24; // column-header height (px)
let HW = HEADER_W;
let HH = HEADER_H;
// Outline gutter: the strip left of the row headers / above the column headers
// holding the group rails and their collapse toggles. Zero-width unless the
// sheet actually has an outline, so a normal sheet is unaffected.
const OUTLINE_STEP = 11; // px of indent per nesting level
let GW = 0;
let GH = 0;
let outlineRowMax = 0;
let outlineColMax = 0;
let outlineToggles = []; // [{x,y,w,h,index,columns}] rebuilt each frame
// Re-read the active sheet's header visibility. Called at the top of measure(),
// so every frame lays out against the current setting.
function syncHeaderMetrics() {
  let hidden = false;
  try { hidden = !!(wasm && wasm.session_headers_hidden(state.sheet)); } catch {}
  // The deepest nesting on each axis sizes its gutter. Asking for zero lines
  // returns just the maximum, so this costs nothing on a sheet with no outline.
  outlineRowMax = outlineColMax = 0;
  if (wasm && !hidden) {
    try {
      outlineRowMax = JSON.parse(wasm.session_outline(state.sheet, 0, 0, false)).max || 0;
      outlineColMax = JSON.parse(wasm.session_outline(state.sheet, 0, 0, true)).max || 0;
    } catch {}
  }
  GW = outlineRowMax ? outlineRowMax * OUTLINE_STEP + 8 : 0;
  GH = outlineColMax ? outlineColMax * OUTLINE_STEP + 8 : 0;
  // HW/HH are the *total* inset the grid starts after, so every existing
  // geometry consumer follows the gutter without knowing it exists. The header
  // strips themselves draw from GW/GH inward.
  HW = hidden ? 0 : HEADER_W + GW;
  HH = hidden ? 0 : HEADER_H + GH;
}
// One indent level, in px — Excel's is about three space-widths.
const INDENT_PX = 10;
// Zoom is applied to the *canvas context*, not to the geometry: the grid keeps
// measuring in engine pixels and only the drawing (and the viewport it has to
// fill) is scaled. That keeps every offset the engine reports directly
// comparable with what is drawn — the alternative, scaling column widths and
// row heights, would put drawn and modelled geometry back out of step.
const ZOOM_MIN = 0.25, ZOOM_MAX = 2;
let COL_W = 64;
let ROW_H = 20;

const state = {
  sheet: 0,
  scrollX: 0, // absolute content pixel offset (left of the viewport)
  scrollY: 0, // absolute content pixel offset (top of the viewport)
  firstRow: 0, // first visible row (derived from scrollY in measure())
  zoom: 1,     // canvas magnification (Ctrl+wheel / View ▸ Zoom)
  firstCol: 0, // first visible column (derived from scrollX in measure())
  sel: { row: 0, col: 0 }, // focus cell
  anchor: { row: 0, col: 0 }, // selection anchor
  selKind: "cells", // "cells" | "rows" | "cols" | "all"
  ranges: [], // committed extra rectangles for a multi-range (Ctrl+click) selection
  dragging: false,
  headerDrag: null, // "row" | "col" | null — which axis a header drag extends
  editing: false,
  resize: null, // active header resize: { axis:"col"|"row", index, previewPx, scope }
  freezeDrag: null, // active freeze-divider drag: { axis:"col"|"row", px, py }
  fill: null, // active drag-fill: { src:{r0,c0,r1,c1}, dst:{...} }
};
let fillHandleRect = null; // screen rect of the fill handle (for hit-testing)
let validationChevron = null; // {x,y,w,h,values} of the active cell's list-dropdown button
let commentCells = new Set(); // "r,c" of cells with a note in view (for hover tooltip)
let errorCells = new Set();   // "r,c" of cells holding an error value, likewise

// What each spreadsheet error actually means, in the terms that caused it.
// "#VALUE!" alone tells you something broke; it does not tell you what to look
// for, which is the whole difficulty of fixing someone else's sheet.
const ERROR_HELP = {
  "#DIV/0!": "Dividing by zero, or by an empty cell.",
  "#VALUE!": "A value has the wrong type — often text where a number is expected.",
  "#REF!": "A reference points at cells that no longer exist (deleted rows or columns).",
  "#NAME?": "An unrecognised name — check the function spelling, or a missing defined name.",
  "#NUM!": "A number the calculation can't represent — out of range, or not a real result.",
  "#N/A": "A lookup found no match.",
  "#NULL!": "Two ranges that were intersected don't overlap.",
  "#SPILL!": "A result needs more room than is free.",
};
let dragPos = null; // latest pointer {px,py} during a selection/fill drag
let autoRaf = 0; // rAF handle for edge auto-scroll while dragging

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
const selStats = document.getElementById("sel-stats");
const vscroll = document.getElementById("vscroll");
const vthumb = document.getElementById("vthumb");
const hscroll = document.getElementById("hscroll");
const hthumb = document.getElementById("hthumb");
const fInput = document.getElementById("formula-input");
const cellRef = document.getElementById("cell-ref");
const commentTip = document.getElementById("comment-tip");
const status = document.getElementById("tb-status");

const css = (name) => getComputedStyle(document.body).getPropertyValue(name).trim();
let colors = {};
function readColors() {
  colors = {
    bg: css("--bg") || "#fff",
    fg: css("--fg") || "#0b0d12",
    muted: css("--muted") || "#7b8391",
    grid: css("--grid") || "#f0f1f4",
    headerBg: css("--surface") || "#f6f7f9",
    accent: css("--accent") || "#2f6df6",
    sel: css("--sel-tint") || "rgba(47,109,246,.10)",
    // Distinct from the selection tint: a find hit and the active cell must not
    // read as the same thing.
    findHit: css("--find-tint") || "rgba(245,158,11,.28)",
    // Read from the theme rather than hardcoded: the freeze divider sits on the
    // grid, so it has to darken and lighten with it. `colors.freezeLine` was
    // already consulted at the draw site but never populated here, so the
    // fallback was always what showed.
    freezeLine: css("--freeze-line") || "#5f6368",
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
let geoItems = []; // cells for the visible window, fetched in measure(), reused by draw()
let sheetMerges = []; // merged ranges of the active sheet, refreshed each draw
let dragTab = -1; // index of the sheet tab being dragged (reorder)

// The height a cell needs, or null if it cannot grow its row. Shared by the
// per-frame measure and the document-wide growth map below — if these two ever
// disagreed, the drawn rows and the scroll offsets would disagree with them.
function neededRowHeight(it, colWidth) {
  // Rotated text needs vertical room, or it is clipped to a 20 px row and the
  // rotation achieves nothing. Height is the text's own length projected onto
  // the vertical axis; stacked text is one glyph per line.
  if (it.rot) {
    ctx.font = cellFont(it);
    let needed;
    if (it.rot === 255) {
      needed = [...String(it.t)].length * cellLineH(it) + 6;
    } else {
      const deg = it.rot <= 90 ? it.rot : it.rot - 90;
      needed = Math.abs(Math.sin((deg * Math.PI) / 180)) * ctx.measureText(String(it.t)).width
        + cellPx(it) + 6;
    }
    return Math.min(needed, 409); // Excel's row-height ceiling
  }
  if (it.w) return wrapLines(it, colWidth - 8).length * cellLineH(it) + 6;
  // A tall font grows its row by the font's own box plus Excel's leading; at the
  // 11 pt default this comes to exactly the default row height, so an ordinary
  // styled row is left alone instead of being inflated by 25%.
  if (it.fs) return cellPx(it) + 5;
  return null;
}

// --- Auto-height growth, document-wide ------------------------------------
//
// A wrapped or rotated cell makes its row taller than the height the engine
// knows about. Measuring text is the host's job, so the engine cannot account
// for it — and when only the *visible* rows were measured, everything derived
// from engine offsets was wrong past the first grown row: scroll anchoring
// hitched at row boundaries, the scrollbar extent came up short, scroll-into-view
// under-scrolled so the selection crept off screen, and a resize drag started
// offset by the accumulated growth.
//
// So the growth is computed for every candidate row on the sheet, once, and
// folded into the offsets through `rowOffsetPx` / `rowAtPx`. Rebuilt whenever
// the document, zoom or column widths change.
let growthRows = [];   // ascending row indices that grow
// growthPrefix[i] = summed growth of the first i entries, so it has one more
// element than growthRows and growthPrefix[n] is the total.
let growthPrefix = [0];
let growthTotal = 0;
let growthDirty = true;

function invalidateGrowth() { growthDirty = true; }

function rebuildGrowth() {
  growthDirty = false;
  growthRows = [];
  growthPrefix = [0];
  growthTotal = 0;
  if (!wasm) return;
  let payload;
  try { payload = JSON.parse(wasm.session_autofit_candidates(state.sheet)); } catch { return; }
  const base = payload.default || 20;
  const extra = new Map();
  // Column widths are looked up once per distinct column, not once per cell: a
  // wrapped column of ten thousand rows is one lookup, not ten thousand.
  const widths = new Map();
  for (const it of payload.cells || []) {
    // Column width matters only for wrapping, and comes from the engine rather
    // than the drawn window — a candidate may be far outside the viewport.
    let cw = widths.get(it.c);
    if (cw === undefined) { cw = colWidthOf(it.c); widths.set(it.c, cw); }
    const needed = neededRowHeight(it, cw);
    if (needed === null || needed <= base) continue;
    const grow = needed - base;
    if (grow > (extra.get(it.r) || 0)) extra.set(it.r, grow);
  }
  growthRows = [...extra.keys()].sort((a, b) => a - b);
  let acc = 0;
  for (const r of growthRows) {
    acc += extra.get(r);
    growthPrefix.push(acc);
  }
  growthTotal = acc;
  if (payload.truncated) {
    // Never silently: a partial map means offsets past the cut are wrong again.
    status.textContent = "too many auto-height rows to measure — scrolling may drift";
  }
}

// A column's width from the engine, cached per frame-independent lookup.
function colWidthOf(col) {
  const drawn = geo.colOf.has(col) ? geo.colW[geo.colOf.get(col)] : undefined;
  if (drawn !== undefined && drawn > 0) return drawn;
  try { return JSON.parse(wasm.session_col_px(state.sheet, col, 1))[0] ?? COL_W; }
  catch { return COL_W; }
}

// Total growth of rows strictly before `row`.
function growthBefore(row) {
  if (growthDirty) rebuildGrowth();
  if (!growthRows.length) return 0;
  // How many grown rows sit strictly above `row`.
  let lo = 0, hi = growthRows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (growthRows[mid] < row) lo = mid + 1; else hi = mid;
  }
  return growthPrefix[lo];
}

// Effective offset of a row's top edge: what the engine says, plus the growth of
// every grown row above it.
function rowOffsetPx(row) {
  return wasm.session_row_offset_px(state.sheet, row) + growthBefore(row);
}

// Inverse of `rowOffsetPx`. Growth is monotonic, so subtracting the growth above
// the current guess and re-asking the engine converges in a couple of steps.
function rowAtPx(px) {
  if (growthDirty) rebuildGrowth();
  let row = wasm.session_row_at_px(state.sheet, Math.round(px));
  for (let i = 0; i < 4; i++) {
    const next = wasm.session_row_at_px(state.sheet, Math.round(px - growthBefore(row)));
    if (next === row) break;
    row = next;
  }
  return row;
}

// Absolute screen position of a column's left / row's top edge (any index).
function screenX(col) { return wasm.session_col_offset_px(state.sheet, col) - state.scrollX + HW; }
function screenY(row) { return rowOffsetPx(row) - state.scrollY + HH; }
// Freeze-aware screen position: frozen lines (index < fc/fr) are pinned and
// ignore the scroll offset; body lines subtract it. In the no-freeze case this
// equals screenX/screenY. Used where geometry must line up under frozen panes
// (merged-cell painting spans across cells, so it can't use per-cell geo).
// Drawn geometry wins where it exists: measure() grows rows for wrapped text
// and tall fonts (auto row height), which the engine's offsets know nothing
// about. Mixing the two made a merge drift from its own cells by the accumulated
// growth — and since that growth depends on which rows are in the window, the
// drift changed on every scroll step and the block appeared to slide over the
// grid. Engine offsets remain the fallback for lines outside the window.
function fscreenX(col) {
  const x = colXAt(col);
  if (x !== undefined) return x;
  const f = state.freeze || { fc: 0 };
  const o = wasm.session_col_offset_px(state.sheet, col);
  return HW + (col < f.fc ? o : o - state.scrollX);
}
function fscreenY(row) {
  const y = rowYAt(row);
  if (y !== undefined) return y;
  const f = state.freeze || { fr: 0 };
  const o = rowOffsetPx(row);
  return HH + (row < f.fr ? o : o - state.scrollY);
}
// The trailing (right/bottom) edge of a line: from the drawn geometry when the
// line itself is drawn, else the start of the following line.
const fscreenXEnd = (col) => (colXAt(col) !== undefined ? colXAt(col) + colWAt(col) : fscreenX(col + 1));
const fscreenYEnd = (row) => (rowYAt(row) !== undefined ? rowYAt(row) + rowHAt(row) : fscreenY(row + 1));
// The merge covering (row,col), if any.
function mergeAt(row, col) {
  return sheetMerges.find((m) => row >= m.r0 && row <= m.r1 && col >= m.c0 && col <= m.c1);
}
// Whether a merge intersects the current (effective) selection.
function mergeInSel(m) {
  const s = effectiveRange();
  return !(m.r1 < s.r0 || m.r0 > s.r1 || m.c1 < s.c0 || m.c0 > s.c1);
}

// The canvas font string for a cell (family + size from its style, or defaults).
// Font size in px: the cell's own size (pt→px at 96dpi), else the 11pt default
// the toolbar reports for an unstyled cell (kept in sync so "11" isn't a lie).
function cellPx(it) { return Math.round(((it.fs || 11) * 4) / 3); }
// Cache the CSS font stack per requested family. font_css_stack (wasm) routes a
// name through the shared substitution table (Calibri→Carlito, Arial→Liberation
// Sans, …) + the bundled @font-face fonts, so a cell's font renders as its
// metric-compatible face on every machine instead of silently falling back to
// the system font. Cached because draw() asks per cell, but families repeat.
const _fontStackCache = new Map();
function fontStack(fn) {
  const key = fn || "";
  let s = _fontStackCache.get(key);
  if (s === undefined) {
    try { s = wasm.font_css_stack(key); } catch { s = "system-ui, sans-serif"; }
    _fontStackCache.set(key, s);
  }
  return s;
}
// The font families the toolbar picker offers, from the engine's substitution
// table (single source of truth) — every entry renders as a bundled face, so
// the list can never offer a family this build would have to guess at. Typed
// names outside the list still work; they go through the same table.
let _fontFamilies = null;
function fontFamilies() {
  if (!_fontFamilies) {
    try { _fontFamilies = JSON.parse(wasm.font_families()); } catch { _fontFamilies = []; }
  }
  return _fontFamilies;
}
function cellFont(it) {
  const weight = it.b ? "600 " : "";
  const slant = it.i ? "italic " : "";
  return `${slant}${weight}${cellPx(it)}px ${fontStack(it.fn)}`;
}
function cellLineH(it) { return cellPx(it) + 4; }
// Baseline y for a single line given the cell's vertical alignment.
function textY(it, yTop, h, lineH) {
  if (it.va === "t") return yTop + lineH / 2 + 2;
  if (it.va === "b") return yTop + h - lineH / 2 - 2;
  return yTop + h / 2;
}

// Word-wrap a cell's text to `maxW` px (hard-breaking over-long words).
function wrapLines(it, maxW) {
  // An explicit line break (Alt+Enter) is a hard break: wrap each of its
  // segments separately rather than letting \s+ swallow it as a space.
  const text = String(it.t);
  if (text.includes("\n")) {
    return text.split("\n").flatMap((seg) => wrapLines({ ...it, t: seg }, maxW));
  }
  ctx.font = cellFont(it);
  const lines = [];
  let line = "";
  for (const word of text.split(/\s+/)) {
    const test = line ? line + " " + word : word;
    if (ctx.measureText(test).width <= maxW || !line) {
      if (!line && ctx.measureText(word).width > maxW) {
        let chunk = "";
        for (const ch of word) {
          if (chunk && ctx.measureText(chunk + ch).width > maxW) { lines.push(chunk); chunk = ch; }
          else chunk += ch;
        }
        line = chunk;
      } else line = test;
    } else { lines.push(line); line = word; }
  }
  if (line) lines.push(line);
  return lines.length ? lines : [""];
}

// Measure the wrap, (re)build `geo` from engine sizes at the current pixel
// scroll, and return `{ w, h }`. Scrolling is fluid: `scrollX/scrollY` are
// absolute content offsets, so the first visible line can be partially clipped.
function measure() {
  syncHeaderMetrics();
  const rect = wrap.getBoundingClientRect();
  const v = { w: rect.width / state.zoom, h: rect.height / state.zoom };
  if (!wasm) {
    geo.colW = geo.colX = geo.colIdx = geo.rowH = geo.rowY = geo.rowIdx = [];
    geo.colOf = new Map(); geo.rowOf = new Map();
    geo.cols = geo.rows = 0;
    state.freeze = { fc: 0, fr: 0, bodyX0: HW, bodyY0: HH };
    return v;
  }
  // Frozen panes: the top `fr` rows and left `fc` columns stay pinned; the rest
  // scroll. The drawn line list is [frozen 0..fc-1 at fixed x] + [scrolling fsc..].
  const fz = JSON.parse(wasm.session_frozen(state.sheet));
  const fc = fz.cols, fr = fz.rows;
  const frozenW = fc > 0 ? wasm.session_col_offset_px(state.sheet, fc) : 0;
  const frozenH = fr > 0 ? rowOffsetPx(fr) : 0;
  const bodyX0 = HW + frozenW, bodyY0 = HH + frozenH;
  state.freeze = { fc, fr, bodyX0, bodyY0 };

  // First scrolling line + its sub-pixel clip. scrollX/Y move the scrolling
  // region, whose content origin is column `fc` / row `fr`.
  const absX = frozenW + state.scrollX;
  const fsc = Math.max(fc, wasm.session_col_at_px(state.sheet, Math.round(absX)));
  const subX = absX - wasm.session_col_offset_px(state.sheet, fsc);
  const absY = frozenH + state.scrollY;
  const fsr = Math.max(fr, rowAtPx(absY));
  const subY = absY - rowOffsetPx(fsr);
  state.firstCol = fsc;
  state.firstRow = fsr;

  const colCap = Math.max(4, Math.ceil((v.w - bodyX0) / MIN_LINE) + 2);
  const rowCap = Math.max(4, Math.ceil((v.h - bodyY0) / MIN_LINE) + 2);
  const frozenColW = fc > 0 ? JSON.parse(wasm.session_col_px(state.sheet, 0, fc)) : [];
  const scrollColW = JSON.parse(wasm.session_col_px(state.sheet, fsc, colCap));
  geo.colIdx = []; geo.colW = [];
  for (let c = 0; c < fc; c++) { geo.colIdx.push(c); geo.colW.push(frozenColW[c]); }
  for (let i = 0; i < scrollColW.length; i++) { geo.colIdx.push(fsc + i); geo.colW.push(scrollColW[i]); }
  const frozenRowH = fr > 0 ? JSON.parse(wasm.session_row_px(state.sheet, 0, fr)) : [];
  const scrollRowH = JSON.parse(wasm.session_row_px(state.sheet, fsr, rowCap));
  geo.rowIdx = []; geo.rowH = [];
  for (let r = 0; r < fr; r++) { geo.rowIdx.push(r); geo.rowH.push(frozenRowH[r]); }
  for (let i = 0; i < scrollRowH.length; i++) { geo.rowIdx.push(fsr + i); geo.rowH.push(scrollRowH[i]); }

  // Live resize preview: override affected line sizes (matched by real index).
  if (state.resize) {
    const rz = state.resize;
    const idxArr = rz.axis === "col" ? geo.colIdx : geo.rowIdx;
    const wArr = rz.axis === "col" ? geo.colW : geo.rowH;
    for (let i = 0; i < idxArr.length; i++) {
      const idx = idxArr[i];
      const hit = rz.scope === "all" || (rz.scope === "band" ? idx >= rz.b0 && idx <= rz.b1 : idx === rz.index);
      if (hit) wArr[i] = rz.previewPx;
    }
  }

  // Index → slot maps (needed by the accessors and auto-height below).
  geo.colOf = new Map(); geo.colIdx.forEach((c, i) => geo.colOf.set(c, i));
  geo.rowOf = new Map(); geo.rowIdx.forEach((r, i) => geo.rowOf.set(r, i));

  // Fetch the visible cells once (covering the frozen bands), reused by draw,
  // and grow rows that contain wrapped text / tall fonts (auto row height).
  const lastRowIdx = geo.rowIdx[geo.rowIdx.length - 1] ?? fsr;
  const lastColIdx = geo.colIdx[geo.colIdx.length - 1] ?? fsc;
  geoItems = JSON.parse(
    wasm.session_cells(state.sheet, fr > 0 ? 0 : fsr, fc > 0 ? 0 : fsc, lastRowIdx, lastColIdx),
  );
  // Rows the workbook sized itself are pinned: auto-height must not override an
  // imported height (nor one the user dragged), exactly as Excel stops
  // auto-fitting a row once its height is set. Without this every styled row of
  // an opened file was silently re-heighted by the editor.
  // (geo.rowIdx is the frozen band followed by the scrolled body, so ask per
  // contiguous run rather than over the gap between them.)
  const pinnedRows = new Set();
  for (let i = 0; i < geo.rowIdx.length; ) {
    let j = i;
    while (j + 1 < geo.rowIdx.length && geo.rowIdx[j + 1] === geo.rowIdx[j] + 1) j++;
    const flags = JSON.parse(wasm.session_row_pinned(state.sheet, geo.rowIdx[i], j - i + 1));
    flags.forEach((p, k) => { if (p) pinnedRows.add(geo.rowIdx[i] + k); });
    i = j + 1;
  }
  for (const it of geoItems) {
    if (!it.t) continue;
    const ci = geo.colOf.get(it.c), ri = geo.rowOf.get(it.r);
    if (ci === undefined || ri === undefined) continue;
    // A hidden row is 0 px — growing it would make it reappear.
    if (geo.rowH[ri] <= 0 || pinnedRows.has(it.r)) continue;
    const needed = neededRowHeight(it, geo.colW[ci]);
    if (needed !== null && needed > geo.rowH[ri]) geo.rowH[ri] = needed;
  }

  // Positions: frozen lines from HW/HH, scrolling lines from bodyX0/Y0 − sub.
  geo.colX = new Array(geo.colIdx.length);
  let x = HW; geo.cols = 0;
  for (let i = 0; i < geo.colIdx.length; i++) {
    if (i === fc) x = bodyX0 - subX;
    geo.colX[i] = x;
    if (x < v.w) geo.cols = i + 1;
    x += geo.colW[i] ?? COL_W; // hidden lines are 0 — must NOT fall back to COL_W
  }
  geo.rowY = new Array(geo.rowIdx.length);
  let y = HH; geo.rows = 0;
  for (let i = 0; i < geo.rowIdx.length; i++) {
    if (i === fr) y = bodyY0 - subY;
    geo.rowY[i] = y;
    if (y < v.h) geo.rows = i + 1;
    y += geo.rowH[i] ?? ROW_H; // hidden lines are 0 — must NOT fall back to ROW_H
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
  ctx.setLineDash(style === "dashed" || style === "mediumDashed" ? [4, 2] : style === "dotted" ? [1, 2] : []);
  // A double border is two thin parallel lines, not one thick one — drawing it
  // heavy makes it indistinguishable from `thick`, which is the very thing it
  // is chosen over for a totals rule. Offset perpendicular to the edge.
  if (style === "double") {
    ctx.lineWidth = 1;
    const vertical = Math.abs(x1 - x0) < Math.abs(y1 - y0);
    for (const d of [-1, 1]) {
      const dx = vertical ? d : 0;
      const dy = vertical ? 0 : d;
      ctx.beginPath();
      ctx.moveTo(Math.floor(x0) + dx + 0.5, Math.floor(y0) + dy + 0.5);
      ctx.lineTo(Math.floor(x1) + dx + 0.5, Math.floor(y1) + dy + 0.5);
      ctx.stroke();
    }
    ctx.setLineDash([]);
    return;
  }
  ctx.lineWidth = width;
  // A 1px line lands crisply on a half-pixel; wider lines centre on the edge.
  const off = width === 1 ? 0.5 : 0;
  ctx.beginPath();
  ctx.moveTo(Math.floor(x0) + off, Math.floor(y0) + off);
  ctx.lineTo(Math.floor(x1) + off, Math.floor(y1) + off);
  ctx.stroke();
  ctx.setLineDash([]);
}

// Size + position the custom scrollbar thumbs from the current scroll and the
// used extent (plus a buffer so you can always scroll a little past the data).
let scrollMeta = { maxScrollY: 1, maxScrollX: 1, vSpan: 1, hSpan: 1 };
function updateScrollbars(v) {
  if (!wasm) return;
  const b = usedBounds();
  const viewH = v.h - HH, viewW = v.w - HW;
  const contentH = Math.max(
    rowOffsetPx(b.rows + 30),
    state.scrollY + viewH + 1,
  );
  const contentW = Math.max(
    wasm.session_col_offset_px(state.sheet, b.cols + 8),
    state.scrollX + viewW + 1,
  );
  const trackH = vscroll.clientHeight, trackW = hscroll.clientWidth;
  const thumbH = Math.max(28, trackH * Math.min(1, viewH / contentH));
  const thumbW = Math.max(28, trackW * Math.min(1, viewW / contentW));
  const maxScrollY = Math.max(1, contentH - viewH);
  const maxScrollX = Math.max(1, contentW - viewW);
  const vSpan = Math.max(1, trackH - thumbH), hSpan = Math.max(1, trackW - thumbW);
  vthumb.style.height = thumbH + "px";
  vthumb.style.top = Math.min(vSpan, (state.scrollY / maxScrollY) * vSpan) + "px";
  hthumb.style.width = thumbW + "px";
  hthumb.style.left = Math.min(hSpan, (state.scrollX / maxScrollX) * hSpan) + "px";
  // Hide a scrollbar when there's nothing to scroll on that axis.
  vscroll.style.display = contentH > viewH + 1 ? "block" : "none";
  hscroll.style.display = contentW > viewW + 1 ? "block" : "none";
  scrollMeta = { maxScrollY, maxScrollX, vSpan, hSpan };
}

// A header boundary under the pointer, if any (for resize hit-testing).
function boundaryAt(px, py) {
  if (py < HH && px >= HW) {
    for (let i = 0; i < geo.colX.length; i++) {
      if (Math.abs(px - (geo.colX[i] + geo.colW[i])) <= RESIZE_GRAB)
        return { axis: "col", index: geo.colIdx[i] };
    }
  } else if (px < HW && py >= HH) {
    for (let i = 0; i < geo.rowY.length; i++) {
      if (Math.abs(py - (geo.rowY[i] + geo.rowH[i])) <= RESIZE_GRAB)
        return { axis: "row", index: geo.rowIdx[i] };
    }
  }
  return null;
}

// Screen position/size of a drawn column/row (or default/undefined if not drawn).
const colWAt = (col) => (geo.colOf.has(col) ? geo.colW[geo.colOf.get(col)] : COL_W);
const rowHAt = (row) => (geo.rowOf.has(row) ? geo.rowH[geo.rowOf.get(row)] : ROW_H);
const colXAt = (col) => (geo.colOf.has(col) ? geo.colX[geo.colOf.get(col)] : undefined);
const rowYAt = (row) => (geo.rowOf.has(row) ? geo.rowY[geo.rowOf.get(row)] : undefined);
const firstDrawnCol = () => geo.colIdx[0] ?? 0;
const firstDrawnRow = () => geo.rowIdx[0] ?? 0;
// The first drawn line of the *scrolling body* — past the frozen band, which
// occupies the first fc/fr entries of colIdx/rowIdx.
const firstBodyCol = () => geo.colIdx[state.freeze.fc] ?? state.firstCol;
const firstBodyRow = () => geo.rowIdx[state.freeze.fr] ?? state.firstRow;

// The clipped [x, x+w) pixel span covering columns c0..c1, kept inside the pane
// those columns belong to. Both limits are pane-relative, which matters twice
// over when something is frozen: with fractional scroll the first body column
// is drawn a sliver *behind* the freeze line (so an unclamped span paints a
// strip into the frozen band that slides as you scroll), and a range whose
// start has scrolled out must clamp to the body's left edge rather than
// collapse to zero width (which made the whole selection disappear).
function spanX(c0, c1, v) {
  const f = state.freeze;
  const lo = c0 < f.fc ? HW : f.bodyX0;  // left edge of c0's pane
  const hi = c1 < f.fc ? f.bodyX0 : v.w; // right edge of c1's pane
  const first = c0 < f.fc ? firstDrawnCol() : firstBodyCol();
  const xa = colXAt(c0), xb = colXAt(c1);
  const left = xa !== undefined ? xa : c0 < first ? lo : hi;
  const right = xb !== undefined ? xb + colWAt(c1) : c1 < first ? lo : hi;
  const x = Math.max(lo, left);
  return { x, w: Math.max(0, Math.min(right, hi) - x) };
}

function spanY(r0, r1, v) {
  const f = state.freeze;
  const lo = r0 < f.fr ? HH : f.bodyY0;
  const hi = r1 < f.fr ? f.bodyY0 : v.h;
  const first = r0 < f.fr ? firstDrawnRow() : firstBodyRow();
  const ya = rowYAt(r0), yb = rowYAt(r1);
  const top = ya !== undefined ? ya : r0 < first ? lo : hi;
  const bot = yb !== undefined ? yb + rowHAt(r1) : r1 < first ? lo : hi;
  const y = Math.max(lo, top);
  return { y, h: Math.max(0, Math.min(bot, hi) - y) };
}

// The clip rect of the quadrant a cell belongs to (whole body when no freeze).
function quadClip(row, col, v) {
  const f = state.freeze;
  const x0 = col < f.fc ? HW : f.bodyX0, x1 = col < f.fc ? f.bodyX0 : v.w;
  const y0 = row < f.fr ? HH : f.bodyY0, y1 = row < f.fr ? f.bodyY0 : v.h;
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

// --- App-header collapse ----------------------------------------------------
// The header bar (branding, status, Open, Settings) is 52px of chrome that a
// user working in a large sheet may not want. Collapsing hands that space to
// the grid; the toggle stays in the menu bar, which is never hidden, so there
// is always a way back. Remembered across sessions — it is a workspace
// preference, not a property of the document.
const HEADER_COLLAPSE_KEY = "oc.headerCollapsed";
let headerCollapsed = false;
function setHeaderCollapsed(collapsed) {
  headerCollapsed = collapsed;
  const hdr = document.querySelector(".app-header");
  const btn = document.getElementById("hdr-collapse");
  // A class, not the `hidden` attribute: `.app-header { display: flex }` is a
  // stronger rule than the UA's `[hidden] { display: none }`, so the attribute
  // alone leaves the bar on screen.
  if (hdr) hdr.classList.toggle("collapsed", collapsed);
  if (btn) {
    btn.classList.toggle("collapsed", collapsed);
    btn.setAttribute("aria-expanded", collapsed ? "false" : "true");
    const label = collapsed ? "Show the page header" : "Hide the page header";
    btn.title = label;
    btn.setAttribute("aria-label", label);
  }
  try { localStorage.setItem(HEADER_COLLAPSE_KEY, collapsed ? "1" : "0"); } catch {}
  resize(); // the canvas re-fits into (or out of) the reclaimed space
}

// Set the magnification and re-fit the canvas. Clamped to 25–200%, Excel's
// range; 100% is exact so Ctrl+0 always lands back on crisp text.
function setZoom(z) {
  const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z * 100) / 100));
  if (next === state.zoom) return;
  state.zoom = next;
  // Text is measured in zoom-logical units, so every grown row's height changes.
  invalidateGrowth();
  resize();
  status.textContent = `zoom ${Math.round(next * 100)}%`;
}

function resize() {
  const rect = wrap.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(rect.width * dpr);
  canvas.height = Math.floor(rect.height * dpr);
  canvas.style.width = rect.width + "px";
  canvas.style.height = rect.height + "px";
  ctx.setTransform(dpr * state.zoom, 0, 0, dpr * state.zoom, 0, 0);
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

  // The pane quadrants (one whole body when nothing is frozen). Selection tint,
  // gridlines and cells are each drawn clipped to their quadrant so frozen and
  // scrolling panes never bleed into one another.
  const F = state.freeze;
  // geo index where body lines begin: the first `fc`/`fr` entries of colIdx/
  // rowIdx are the frozen lines, the rest are the scrolling body.
  const nCol = geo.colX.length, nRow = geo.rowY.length;
  const splitC = Math.min(F.fc, nCol), splitR = Math.min(F.fr, nRow);
  // Each quadrant carries the geo line-index ranges it owns, so gridlines are
  // drawn per pane. This matters because with fractional scroll the first body
  // line sits at bodyX0/Y0 − sub (just inside the frozen band's clip); gating
  // by index keeps that stray line out of the frozen panes.
  const quads = F.fc || F.fr
    ? [
        { x: HW, y: HH, w: F.bodyX0 - HW, h: F.bodyY0 - HH, ci0: 0, ci1: splitC, ri0: 0, ri1: splitR },
        { x: F.bodyX0, y: HH, w: v.w - F.bodyX0, h: F.bodyY0 - HH, ci0: splitC, ci1: nCol, ri0: 0, ri1: splitR },
        { x: HW, y: F.bodyY0, w: F.bodyX0 - HW, h: v.h - F.bodyY0, ci0: 0, ci1: splitC, ri0: splitR, ri1: nRow },
        { x: F.bodyX0, y: F.bodyY0, w: v.w - F.bodyX0, h: v.h - F.bodyY0, ci0: splitC, ci1: nCol, ri0: splitR, ri1: nRow },
      ].filter((q) => q.w > 0 && q.h > 0)
    : [{ x: HW, y: HH, w: Math.max(0, v.w - HW), h: Math.max(0, v.h - HH), ci0: 0, ci1: nCol, ri0: 0, ri1: nRow }];

  // Selection tint + gridlines, per quadrant. Grid lines can be hidden per
  // sheet (Excel's showGridLines="0") — the selection tint still paints.
  let gridHidden = false;
  try { gridHidden = wasm && wasm.session_gridlines_hidden(state.sheet); } catch {}
  for (const q of quads) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(q.x, q.y, q.w, q.h);
    ctx.clip();
    ctx.fillStyle = colors.sel;
    ctx.fillRect(sX.x, sY.y, sX.w, sY.h);
    // Extra banked ranges of a multi-range selection get the same tint.
    for (const rg of state.ranges) {
      const ex = spanX(rg.c0, rg.c1, v), ey = spanY(rg.r0, rg.r1, v);
      ctx.fillRect(ex.x, ey.y, ex.w, ey.h);
    }
    if (gridHidden) { ctx.restore(); continue; }
    ctx.strokeStyle = colors.grid;
    ctx.lineWidth = 1;
    ctx.beginPath();
    // Vertical lines: the columns this quadrant owns, spanning its height. Skip
    // the trailing line when it is the body's fractional left edge (ci1===fc),
    // which the freeze divider covers instead — this is what stopped the stray
    // gridline bleeding into a frozen pane.
    for (let i = q.ci0; i <= q.ci1; i++) {
      if (i === q.ci1 && q.ci1 === splitC && (F.fc || F.fr)) continue;
      const x = Math.floor(i < geo.colX.length ? geo.colX[i] : v.w) + 0.5;
      ctx.moveTo(x, q.y);
      ctx.lineTo(x, q.y + q.h);
    }
    for (let i = q.ri0; i <= q.ri1; i++) {
      if (i === q.ri1 && q.ri1 === splitR && (F.fc || F.fr)) continue;
      const y = Math.floor(i < geo.rowY.length ? geo.rowY[i] : v.h) + 0.5;
      ctx.moveTo(q.x, y);
      ctx.lineTo(q.x + q.w, y);
    }
    ctx.stroke();
    ctx.restore();
  }

  // Outer body clip (keeps content out of the header strips). Cells additionally
  // clip to their own quadrant below.
  ctx.save();
  ctx.beginPath();
  ctx.rect(HW, HH, Math.max(0, v.w - HW), Math.max(0, v.h - HH));
  ctx.clip();

  // Cell fills + text (fetched in measure(), reused here).
  const lastCol = (geo.colIdx[geo.colIdx.length - 1] ?? state.firstCol) + 1;
  const items = geoItems;
  sheetMerges = wasm ? JSON.parse(wasm.session_merges(state.sheet)) : [];

  // Clip a cell's drawing to its pane quadrant (a no-op when nothing is frozen).
  const frozen = F.fc || F.fr;
  const withQuad = (row, col, fn) => {
    if (!frozen) { fn(); return; }
    const q = quadClip(row, col, v);
    ctx.save();
    ctx.beginPath();
    ctx.rect(q.x, q.y, q.w, q.h);
    ctx.clip();
    fn();
    ctx.restore();
  };
  // Draw once per pane, clipped to it — for overlays (selection outline, copy
  // marching ants, drag-fill preview) whose rect may legitimately span more
  // than one pane, so clipping to a single cell's quadrant would truncate them
  // while not clipping at all lets them stroke across the frozen bands.
  const perQuad = (fn) => {
    if (!frozen) { fn(); return; }
    for (const q of quads) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(q.x, q.y, q.w, q.h);
      ctx.clip();
      fn();
      ctx.restore();
    }
  };
  // A merge that straddles a freeze line is not one rectangle but one per pane:
  // its frozen half is pinned while its body half scrolls, so a single rect
  // built from a pinned left edge and a scrolling right edge is wrong the
  // moment they disagree — and once the body half scrolls out it goes
  // *negative*, which canvas draws as a block flipped back over the frozen
  // pane, sliding with every wheel tick. Each entry here is one pane's slice:
  // its clip, its own geometry, and whether it holds the merge's anchor cell
  // (the half that carries the text).
  function mergeSlices(m) {
    const clipRect = { x: HW, y: HH, w: v.w - HW, h: v.h - HH };
    if (!frozen) {
      return [{
        clip: clipRect, anchor: true,
        x: fscreenX(m.c0), y: fscreenY(m.r0),
        w: fscreenXEnd(m.c1) - fscreenX(m.c0), h: fscreenYEnd(m.r1) - fscreenY(m.r0),
      }];
    }
    // Per axis: the frozen half spans c0…fc-1 (clamped at the freeze line), the
    // body half spans max(c0,fc)…c1. Each is measured in its own pane's frame.
    const xs = [], ys = [];
    if (m.c0 < F.fc) {
      const x = fscreenX(m.c0);
      xs.push({ x, w: Math.min(fscreenXEnd(Math.min(m.c1, F.fc - 1)), F.bodyX0) - x, c0: HW, c1: F.bodyX0, anchor: true });
    }
    if (m.c1 >= F.fc) {
      const x = fscreenX(Math.max(m.c0, F.fc));
      xs.push({ x, w: fscreenXEnd(m.c1) - x, c0: F.bodyX0, c1: v.w, anchor: m.c0 >= F.fc });
    }
    if (m.r0 < F.fr) {
      const y = fscreenY(m.r0);
      ys.push({ y, h: Math.min(fscreenYEnd(Math.min(m.r1, F.fr - 1)), F.bodyY0) - y, r0: HH, r1: F.bodyY0, anchor: true });
    }
    if (m.r1 >= F.fr) {
      const y = fscreenY(Math.max(m.r0, F.fr));
      ys.push({ y, h: fscreenYEnd(m.r1) - y, r0: F.bodyY0, r1: v.h, anchor: m.r0 >= F.fr });
    }
    const out = [];
    for (const sx of xs) {
      for (const sy of ys) {
        if (sx.w <= 0 || sy.h <= 0 || sx.c1 <= sx.c0 || sy.r1 <= sy.r0) continue;
        out.push({
          clip: { x: sx.c0, y: sy.r0, w: sx.c1 - sx.c0, h: sy.r1 - sy.r0 },
          anchor: sx.anchor && sy.anchor,
          x: sx.x, y: sy.y, w: sx.w, h: sy.h,
        });
      }
    }
    return out;
  }
  const withSliceClip = (s, fn) => {
    ctx.save();
    ctx.beginPath();
    ctx.rect(s.clip.x, s.clip.y, s.clip.w, s.clip.h);
    ctx.clip();
    fn();
    ctx.restore();
  };

  // Data bars sit behind the value: drawn after the cell fills so they read as
  // part of the cell, before the text so they never obscure it.
  ctx.textBaseline = "middle";
  for (const it of items) {
    if (it.bar === undefined) continue;
    const bx = colXAt(it.c), by = rowYAt(it.r);
    if (bx === undefined || by === undefined) continue;
    const bw = Math.max(0, (colWAt(it.c) - 2) * it.bar);
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#" + (it.barc || "638EC6");
      ctx.globalAlpha = 0.45;
      ctx.fillRect(bx + 1, by + 2, bw, rowHAt(it.r) - 4);
      ctx.globalAlpha = 1;
    });
  }
  for (const it of items) {
    if (!it.bg) continue;
    const x = colXAt(it.c);
    const y = rowYAt(it.r);
    if (x === undefined || y === undefined) continue;
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#" + it.bg;
      ctx.fillRect(x + 1, y + 1, colWAt(it.c) - 1, rowHAt(it.r) - 1);
    });
  }
  // The merges touching each drawn row. Built once per frame so the text pass
  // can ask "is this cell inside a merge?" with a lookup over the one or two
  // merges on its row, instead of scanning the whole sheet's merge list for
  // every cell it paints (which made draw() O(visible cells × total merges),
  // on the unthrottled wheel-scroll path).
  const mergesByRow = new Map();
  for (const m of sheetMerges) {
    for (const r of geo.rowIdx) {
      if (r < m.r0 || r > m.r1) continue;
      const list = mergesByRow.get(r);
      if (list) list.push(m);
      else mergesByRow.set(r, [m]);
    }
  }
  const drawnMergeAt = (r, c) => mergesByRow.get(r)?.find((m) => c >= m.c0 && c <= m.c1);

  // Cells that hold text — these block a neighbor's overflow. A merged block is
  // occupied over its whole span (not just its anchor), or a neighbour's text
  // would spill across it.
  const occupied = new Set();
  for (const it of items) if (it.t) occupied.add(it.r + "," + it.c);
  for (const [r, list] of mergesByRow) {
    for (const c of geo.colIdx) {
      if (list.some((m) => c >= m.c0 && c <= m.c1)) occupied.add(r + "," + c);
    }
  }
  // Cells carrying centre-across, so a label can find the extent of its run.
  const contCols = new Set();
  for (const it of items) if (it.a === "cont") contCols.add(it.r + "," + it.c);
  for (const it of items) {
    if (!it.t) continue;
    // Merged text belongs to the merge pass below, which lays it out across the
    // whole block. Drawing it here too anchors right/centre alignment to the
    // *anchor cell* instead, leaving a fragment outside the block that the
    // merge pass never covers — a ghost of the label beside it.
    if (drawnMergeAt(it.r, it.c)) continue;
    const x = colXAt(it.c);
    const yTop = rowYAt(it.r);
    if (x === undefined || yTop === undefined) continue;
    const w = colWAt(it.c);
    const h = rowHAt(it.r);
    const y = textY(it, yTop, h, cellLineH(it));
    ctx.font = cellFont(it);
    // `it.a` carries the OOXML mode, not just an edge: fill, justify,
    // centre-across and distributed all need their own layout. `align` is the
    // edge each of them starts from.
    const mode = it.a;
    const align =
      mode === "r" ? "right" : mode === "c" || mode === "cont" ? "center" : "left";

    // Wrapped cells: multi-line, clipped to the (auto-grown) cell — no overflow.
    if (it.w) {
      const lh = cellLineH(it);
      const lines = wrapLines(it, w - 8);
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, w, h);
      ctx.clip();
      ctx.font = cellFont(it);
      ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
      // Indent shifts the text off its leading edge (Excel: ~3 space-widths
      // per level), so an indented label lines up under its parent.
      const ind = (it.in || 0) * INDENT_PX;
      const tx = align === "right" ? x + w - 5 - ind : align === "center" ? x + w / 2 : x + 5 + ind;
      ctx.textAlign = align;
      const block = lines.length * lh;
      // Vertical justify/distribute spread the lines over the cell's height
      // instead of stacking them at one edge. Justify puts the first line hard
      // at the top and the last hard at the bottom; distribute leaves an equal
      // gap outside them too.
      const spread = it.va === "vj" || it.va === "vd";
      const slack = Math.max(0, h - 6 - block);
      const gaps = it.va === "vd" ? lines.length + 1 : Math.max(1, lines.length - 1);
      const step = spread && lines.length > 1 ? slack / gaps : 0;
      let ly =
        (spread
          ? yTop + 3 + (it.va === "vd" ? step : 0)
          : it.va === "t"
            ? yTop + 3
            : it.va === "b"
              ? yTop + h - block - 3
              : yTop + Math.max(0, (h - block) / 2)) + lh / 2;
      const stretch = mode === "just" || mode === "dist";
      lines.forEach((ln, i) => {
        // Justify stretches every line but the last; distributed stretches that
        // one too, which is the only difference between the two.
        const last = i === lines.length - 1;
        if (stretch && !(last && mode === "just")) {
          drawStretched(ln, x + 5 + ind, w - 10 - ind, ly);
        } else {
          ctx.fillText(ln, tx, ly);
        }
        ly += lh + step;
      });
      ctx.restore();
      continue;
    }
    // Rotated text: draw it under a transform anchored at the cell's centre and
    // clipped to the cell. OOXML's encoding is 0-90 counter-clockwise, 91-180 for
    // (value - 90) clockwise, and 255 for letters stacked without rotating.
    if (it.rot) {
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, w, h);
      ctx.clip();
      ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
      ctx.textAlign = "center";
      if (it.rot === 255) {
        // Stacked: one glyph per line, upright.
        const lh = cellLineH(it);
        const chars = [...String(it.t)];
        let sy = yTop + lh / 2 + 2;
        for (const ch of chars) {
          if (sy > yTop + h) break;
          ctx.fillText(ch, x + w / 2, sy);
          sy += lh;
        }
      } else {
        const deg = it.rot <= 90 ? -it.rot : it.rot - 90;
        ctx.translate(x + w / 2, yTop + h / 2);
        ctx.rotate((deg * Math.PI) / 180);
        ctx.fillText(String(it.t), 0, 0);
      }
      ctx.restore();
      continue;
    }
    // "Fill" repeats the text until the cell is full — Excel's separator-row
    // idiom. Repeating is the mode, so it happens before any overflow scan.
    if (mode === "fill") {
      const unit = ctx.measureText(String(it.t)).width;
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, w, h);
      ctx.clip();
      ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
      ctx.textAlign = "left";
      if (unit > 0.5) {
        // Guarded on a real width: a zero-width string would loop forever.
        for (let fx = x + 5; fx < x + w; fx += unit) ctx.fillText(String(it.t), fx, y);
      }
      ctx.restore();
      continue;
    }
    // "Center across selection": centre over this cell plus the run of empty
    // cells to its right. It looks like a merge but merges nothing, so the
    // cells underneath stay individually addressable.
    if (mode === "cont") {
      // The span is the run of *cells that also carry the mode* — that is how
      // OOXML encodes the group, one `centerContinuous` per cell. Centring over
      // every empty neighbour instead would fling a lone label into the middle
      // of the viewport.
      let last = it.c;
      const inFroz = F.fc > 0 && it.c < F.fc;
      const hi = inFroz ? F.fc : lastCol;
      while (last + 1 < hi && geo.colOf.has(last + 1) && contCols.has(it.r + "," + (last + 1))) last += 1;
      const spanR = colXAt(last) + colWAt(last);
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, spanR - x, h);
      ctx.clip();
      ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
      ctx.textAlign = "center";
      ctx.fillText(String(it.t), (x + spanR) / 2, y);
      ctx.restore();
      continue;
    }
    let text = it.t;
    let tw = ctx.measureText(text).width;

    // Text overflows across adjacent EMPTY cells (Excel behavior). Extend the
    // clip rectangle left/right over blank neighbours until the text fits or a
    // non-empty cell blocks it.
    //
    // The scan stays inside the cell's own pane: a frozen cell can't borrow the
    // body's columns (they scroll out from under it) and a body cell can't
    // reach back under the frozen band. Bounding by index rather than leaving
    // it to the clip is what makes the *extent* right — the text is then cut at
    // the real boundary instead of being laid out as if the pane went on.
    const inFrozenCols = F.fc > 0 && it.c < F.fc;
    const spillLo = inFrozenCols ? 0 : Math.max(state.firstCol, F.fc);
    const spillHi = inFrozenCols ? F.fc : lastCol; // exclusive
    let clipL = x, clipR = x + w;
    // A number never spills, not even into an empty neighbour: Excel fills the
    // cell with "#" instead, because a number cut off mid-digits still reads as
    // a real — and wrong — value. This holds under "clip" too, for the same
    // reason.
    if (it.n && tw > w - 8) {
      const hashW = ctx.measureText("#").width || 1;
      text = "#".repeat(Math.max(1, Math.floor((w - 8) / hashW)));
      tw = ctx.measureText(text).width;
    // "Clip" (`it.cl`) is the third state of the overflow control: stop at the
    // cell edge rather than borrowing empty neighbours. Excel has no such
    // setting — it always spills — so this skips the spill scan entirely and
    // leaves the span at the cell's own bounds.
    } else if (!it.cl && tw > w - 8) {
      if (align !== "right") {
        let c = it.c;
        // Stop at a non-empty cell OR a column that isn't drawn (e.g. the gap
        // between a frozen band and the scrolling body) — colXAt is undefined
        // there and would make the clip NaN.
        while (clipR - x < tw + 8 && c + 1 < spillHi && geo.colOf.has(c + 1) && !occupied.has(it.r + "," + (c + 1))) {
          c += 1;
          clipR = colXAt(c) + colWAt(c);
        }
      }
      if (align !== "left") {
        let c = it.c;
        while (x + w - clipL < tw + 8 && c - 1 >= spillLo && geo.colOf.has(c - 1) && !occupied.has(it.r + "," + (c - 1))) {
          c -= 1;
          clipL = colXAt(c);
        }
      }
    }

    ctx.save();
    if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
    ctx.beginPath();
    ctx.rect(clipL, yTop, clipR - clipL, h);
    ctx.clip();
    ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
    let tx;
    const ind = (it.in || 0) * INDENT_PX;
    if (align === "right") { ctx.textAlign = "right"; tx = x + w - 5 - ind; }
    else if (align === "center") { ctx.textAlign = "center"; tx = x + w / 2; }
    else { ctx.textAlign = "left"; tx = x + 5 + ind; }
    ctx.fillText(text, tx, y);
    if (it.u || it.st) {
      const lw = Math.min(tw, clipR - clipL - 8);
      const lx = align === "right" ? tx - lw : align === "center" ? tx - lw / 2 : tx;
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = 1;
      if (it.u) { ctx.beginPath(); ctx.moveTo(lx, y + 7.5); ctx.lineTo(lx + lw, y + 7.5); ctx.stroke(); }
      if (it.st) { ctx.beginPath(); ctx.moveTo(lx, y); ctx.lineTo(lx + lw, y); ctx.stroke(); }
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
    withQuad(it.r, it.c, () => {
      drawEdge(it.bd.l, x, yTop, x, yTop + h);
      drawEdge(it.bd.r, x + w, yTop, x + w, yTop + h);
      drawEdge(it.bd.t, x, yTop, x + w, yTop);
      drawEdge(it.bd.b, x, yTop + h, x + w, yTop + h);
      // Diagonals: one line description, drawn along either corner-to-corner
      // direction the cell asks for — a cell can carry both, forming a cross.
      if (it.bd.d) {
        if (it.bd.dd) drawEdge(it.bd.d, x, yTop, x + w, yTop + h);
        if (it.bd.du) drawEdge(it.bd.d, x, yTop + h, x + w, yTop);
      }
    });
  }

  // Merged ranges: paint each as one cell — erase interior gridlines, redraw the
  // top-left cell's fill + text across the span, outline it.
  for (const m of sheetMerges) {
    const it = items.find((t) => t.r === m.r0 && t.c === m.c0);
    const selected = mergeInSel(m);
    for (const s of mergeSlices(m)) {
      const { x: mx, y: my, w: mw, h: mh } = s;
      if (mx > v.w || my > v.h || mx + mw < HW || my + mh < HH) continue;
      withSliceClip(s, () => {
        ctx.fillStyle = it && it.bg ? "#" + it.bg : colors.bg;
        ctx.fillRect(mx, my, mw, mh);
        if (selected) { ctx.fillStyle = colors.sel; ctx.fillRect(mx, my, mw, mh); }
        ctx.strokeStyle = colors.grid;
        ctx.lineWidth = 1;
        ctx.strokeRect(Math.floor(mx) + 0.5, Math.floor(my) + 0.5, Math.round(mw) - 1, Math.round(mh) - 1);
        // The text belongs to the anchor's half; the other half of a straddling
        // merge shows only the fill, as its own pane's slice of the block.
        if (it && it.t && s.anchor) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(mx, my, mw, mh);
          ctx.clip();
          ctx.font = cellFont(it);
          ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
          const al = it.a === "r" ? "right" : it.a === "c" ? "center" : "left";
          ctx.textAlign = al;
          const tx = al === "right" ? mx + mw - 5 : al === "center" ? mx + mw / 2 : mx + 5;
          ctx.fillText(it.t, tx, my + mh / 2);
          ctx.restore();
        }
      });
    }
  }

  // Find highlights: every match on this sheet gets a soft tint, so "3 of 47"
  // is answerable by looking at the sheet rather than by stepping through it.
  // The current match keeps the ordinary selection, which stays distinct.
  if (findState.matches.length && !findBar.hidden) {
    ctx.fillStyle = colors.findHit;
    for (const m of findState.matches) {
      if (m.s !== undefined && m.s !== state.sheet) continue;
      const hx = colXAt(m.c), hy = rowYAt(m.r);
      if (hx === undefined || hy === undefined) continue;
      withQuad(m.r, m.c, () => ctx.fillRect(hx + 1, hy + 1, colWAt(m.c) - 1, rowHAt(m.r) - 1));
    }
  }

  // Range finder: while a formula is being edited, outline each block it
  // references, one colour per reference, matching the order they appear in the
  // text. Drawn under the selection so the active cell still reads as active.
  // A reference to another sheet is not outlined here — it is not on screen.
  if (refSpans.length) {
    ctx.lineWidth = 1.5;
    ctx.setLineDash([]);
    refSpans.forEach((r, i) => {
      if (r.sh) return;
      const rx = spanX(r.c0, r.c1, v), ry = spanY(r.r0, r.r1, v);
      if (rx.w <= 0 || ry.h <= 0) return;
      ctx.strokeStyle = REF_COLORS[i % REF_COLORS.length];
      perQuad(() => ctx.strokeRect(rx.x + 0.75, ry.y + 0.75, rx.w - 1.5, ry.h - 1.5));
    });
  }

  // Range border (multi-cell selections only) + focus-cell border. A single-cell
  // selection is drawn solely by the focus border below, which spans a merge —
  // otherwise this would stroke a box around just the merge's anchor cell,
  // showing a spurious interior line inside the merge.
  ctx.strokeStyle = colors.accent;
  ctx.lineWidth = 2;
  const singleCell = rectSel.r0 === rectSel.r1 && rectSel.c0 === rectSel.c1;
  if (state.selKind === "cells" && !singleCell && sX.w > 0 && sY.h > 0) {
    perQuad(() => ctx.strokeRect(sX.x + 1, sY.y + 1, sX.w - 1, sY.h - 1));
  }
  const fm = mergeAt(state.sel.row, state.sel.col);
  if (fm) {
    // One outline per pane the merge occupies, each in that pane's geometry.
    for (const s of mergeSlices(fm)) {
      withSliceClip(s, () => {
        ctx.strokeStyle = colors.accent;
        ctx.lineWidth = 2;
        ctx.strokeRect(s.x + 1, s.y + 1, s.w - 1, s.h - 1);
      });
    }
  } else {
    withQuad(state.sel.row, state.sel.col, () => {
      ctx.strokeStyle = colors.accent;
      ctx.lineWidth = 2;
      const fx = colXAt(state.sel.col);
      const fy = rowYAt(state.sel.row);
      if (fx !== undefined && fy !== undefined) {
        ctx.strokeRect(fx + 1, fy + 1, colWAt(state.sel.col) - 1, rowHAt(state.sel.row) - 1);
      }
    });
  }

  // Marching-ants outline around the copy/cut source (animated dash offset).
  if (clipMarch && clipMarch.sheet === state.sheet) {
    const mx = spanX(clipMarch.c0, clipMarch.c1, v), my = spanY(clipMarch.r0, clipMarch.r1, v);
    if (mx.w > 1 && my.h > 1) {
      ctx.save();
      ctx.strokeStyle = colors.accent;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([5, 3]);
      ctx.lineDashOffset = -marchOffset;
      perQuad(() => ctx.strokeRect(mx.x + 0.75, my.y + 0.75, mx.w - 1.5, my.h - 1.5));
      ctx.setLineDash([]);
      ctx.restore();
    }
  }

  // Drag-fill preview outline.
  if (state.fill && state.fill.dst) {
    const d = state.fill.dst;
    const dx = spanX(d.c0, d.c1, v), dy = spanY(d.r0, d.r1, v);
    ctx.strokeStyle = colors.accent;
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 3]);
    perQuad(() => ctx.strokeRect(dx.x + 0.5, dy.y + 0.5, dx.w - 1, dy.h - 1));
    ctx.setLineDash([]);
  }

  // Fill handle at the selection's bottom-right corner (cell selections only).
  fillHandleRect = null;
  if (state.selKind === "cells" && !state.fill) {
    const hc = colXAt(rectSel.c1), hr = rowYAt(rectSel.r1);
    if (hc !== undefined && hr !== undefined) {
      const hx = hc + colWAt(rectSel.c1), hy = hr + rowHAt(rectSel.r1);
      withQuad(rectSel.r1, rectSel.c1, () => {
        ctx.fillStyle = colors.accent;
        ctx.fillRect(hx - 3, hy - 3, 6, 6);
        ctx.strokeStyle = colors.bg;
        ctx.lineWidth = 1;
        ctx.strokeRect(hx - 3.5, hy - 3.5, 7, 7);
      });
      fillHandleRect = { x: hx, y: hy };
    }
  }

  // Autofilter header buttons, drawn before the validation chevron so the
  // active cell's own dropdown wins if the two ever land on the same cell.
  refreshFilterInfo();
  drawFilterButtons(withQuad);

  // Data-validation dropdown button on the active cell (if it has a list rule).
  validationChevron = null;
  if (wasm && state.selKind === "cells" && !state.fill) {
    let vals = null;
    try { const vj = wasm.session_validation_at(state.sheet, state.sel.row, state.sel.col); if (vj !== "null") vals = JSON.parse(vj); } catch {}
    const cx = colXAt(state.sel.col), cy = rowYAt(state.sel.row);
    if (vals && cx !== undefined && cy !== undefined) {
      const cw = colWAt(state.sel.col), ch = rowHAt(state.sel.row);
      const bw = 17, bx = cx + cw, by = cy;
      withQuad(state.sel.row, state.sel.col, () => {
        ctx.fillStyle = colors.accent;
        ctx.fillRect(bx, by, bw, ch);
        ctx.strokeStyle = "#fff"; ctx.lineWidth = 1.6; ctx.lineJoin = "round";
        const mx = bx + bw / 2, my = by + ch / 2;
        ctx.beginPath();
        ctx.moveTo(mx - 4, my - 2); ctx.lineTo(mx, my + 2.5); ctx.lineTo(mx + 4, my - 2);
        ctx.stroke();
      });
      validationChevron = { x: bx, y: by, w: bw, h: ch, values: vals };
    }
  }

  // Error indicators — a small marker in the top-left of any cell holding an
  // error value, so a broken formula is findable by eye instead of only by
  // reading every cell. Hovering explains it (see the mousemove handler).
  errorCells = new Set();
  for (const it of items) {
    if (!it.er) continue;
    errorCells.add(it.r + "," + it.c);
    const ex = colXAt(it.c), ey = rowYAt(it.r);
    if (ex === undefined || ey === undefined) continue;
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#e5484d";
      ctx.beginPath();
      ctx.moveTo(ex + 1, ey + 1);
      ctx.lineTo(ex + 8, ey + 1);
      ctx.lineTo(ex + 1, ey + 8);
      ctx.closePath();
      ctx.fill();
    });
  }

  // Comment indicators — a small red triangle in each commented cell's corner.
  commentCells = new Set();
  if (wasm) {
    const r0 = geo.rowIdx[0] ?? state.firstRow, c0 = geo.colIdx[0] ?? state.firstCol;
    const r1 = geo.rowIdx[geo.rowIdx.length - 1] ?? r0, c1 = geo.colIdx[geo.colIdx.length - 1] ?? c0;
    let cmts = [];
    try { cmts = JSON.parse(wasm.session_comments(state.sheet, r0, c0, r1, c1)); } catch {}
    for (const cm of cmts) {
      commentCells.add(cm.r + "," + cm.c);
      const cx = colXAt(cm.c), cy = rowYAt(cm.r);
      if (cx === undefined || cy === undefined) continue;
      const cw = colWAt(cm.c);
      withQuad(cm.r, cm.c, () => {
        ctx.fillStyle = "#e5484d";
        ctx.beginPath();
        ctx.moveTo(cx + cw - 7, cy); ctx.lineTo(cx + cw, cy); ctx.lineTo(cx + cw, cy + 7);
        ctx.closePath(); ctx.fill();
      });
    }
  }
  ctx.restore(); // end body clip


  // Which columns/rows are covered by the current selection (for header tint).
  const rr = selRect();
  const colInSel = (c) => state.selKind === "all" ||
    (state.selKind === "cols" ? c >= rr.c0 && c <= rr.c1 : state.selKind === "cells" && c >= rr.c0 && c <= rr.c1);
  const rowInSel = (r) => state.selKind === "all" ||
    (state.selKind === "rows" ? r >= rr.r0 && r <= rr.r1 : state.selKind === "cells" && r >= rr.r0 && r <= rr.r1);

  // Headers (painted over the body edges), with the selected band tinted.
  // A sheet may hide them (OOXML showRowColHeaders="0"); HW/HH are then 0, the
  // grid runs to the corner, and this whole block is skipped.
  if (HW || HH) {
  ctx.fillStyle = colors.headerBg;
  ctx.fillRect(0, 0, v.w, HH);
  ctx.fillRect(0, 0, HW, v.h);
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  // Column headers, frozen segment then scrolling segment (each clipped so the
  // scrolling headers can't bleed into the frozen band).
  const drawColHeaders = (clipX, clipW, wantFrozen) => {
    if (clipW <= 0) return;
    ctx.save(); ctx.beginPath(); ctx.rect(clipX, GH, clipW, HH - GH); ctx.clip();
    for (let i = 0; i < geo.cols; i++) {
      if (geo.colW[i] <= 0) continue;
      if ((geo.colIdx[i] < F.fc) !== wantFrozen) continue;
      const c = geo.colIdx[i];
      if (colInSel(c)) { ctx.fillStyle = colors.sel; ctx.fillRect(geo.colX[i], GH, geo.colW[i], HH - GH); }
      ctx.fillStyle = colInSel(c) ? colors.accent : colors.muted;
      ctx.fillText(colName(c), geo.colX[i] + geo.colW[i] / 2, GH + (HH - GH) / 2);
    }
    ctx.restore();
  };
  if (F.fc) drawColHeaders(HW, F.bodyX0 - HW, true);
  drawColHeaders(F.bodyX0, v.w - F.bodyX0, false);

  const drawRowHeaders = (clipY, clipH, wantFrozen) => {
    if (clipH <= 0) return;
    ctx.save(); ctx.beginPath(); ctx.rect(GW, clipY, HW - GW, clipH); ctx.clip();
    for (let i = 0; i < geo.rows; i++) {
      if (geo.rowH[i] <= 0) continue;
      if ((geo.rowIdx[i] < F.fr) !== wantFrozen) continue;
      const r = geo.rowIdx[i];
      if (rowInSel(r)) { ctx.fillStyle = colors.sel; ctx.fillRect(GW, geo.rowY[i], HW - GW, geo.rowH[i]); }
      ctx.fillStyle = rowInSel(r) ? colors.accent : colors.muted;
      ctx.fillText(String(r + 1), GW + (HW - GW) / 2, geo.rowY[i] + geo.rowH[i] / 2);
    }
    ctx.restore();
  };
  if (F.fr) drawRowHeaders(HH, F.bodyY0 - HH, true);
  drawRowHeaders(F.bodyY0, v.h - F.bodyY0, false);
  drawHiddenMarkers();
  drawOutlineGutter(v);
  } // end headers
  drawFreezeDividers(v);

  // The in-cell editor is a DOM element over the canvas: keep it on its cell as
  // the grid scrolls or resizes under it (grid-wrap's overflow clips it once the
  // cell leaves the viewport), instead of leaving it parked mid-air.
  if (editSurface === inline) positionInline();
  updateNameBox();
  announceCell();
  updateCellMode();
  updateScrollbars(v);
  updateStats();
  if (wasm) refreshFormulaBar();
  if (wasm && activePanel) refreshPanel();
}

const FREEZE_GRAB = 4; // px proximity to the freeze divider that arms a drag

// Prominent, draggable freeze dividers (Sheets-style), drawn on top of the
// headers. During a drag the line follows the pointer as a live preview.
function drawFreezeDividers(v) {
  const F = state.freeze;
  const drag = state.freezeDrag;
  const showCol = F.fc > 0 || (drag && drag.axis === "col");
  const showRow = F.fr > 0 || (drag && drag.axis === "row");
  if (!showCol && !showRow) return;
  ctx.save();
  const line = colors.freezeLine || "#5f6368";
  if (showCol) {
    const x = drag && drag.axis === "col" ? drag.px : F.bodyX0;
    const g = ctx.createLinearGradient(x, 0, x + 7, 0);
    g.addColorStop(0, "rgba(60,64,72,0.20)"); g.addColorStop(1, "rgba(60,64,72,0)");
    ctx.fillStyle = g; ctx.fillRect(x, 0, 7, v.h);
    ctx.strokeStyle = line; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(x - 1, 0); ctx.lineTo(x - 1, v.h); ctx.stroke();
  }
  if (showRow) {
    const y = drag && drag.axis === "row" ? drag.py : F.bodyY0;
    const g = ctx.createLinearGradient(0, y, 0, y + 7);
    g.addColorStop(0, "rgba(60,64,72,0.20)"); g.addColorStop(1, "rgba(60,64,72,0)");
    ctx.fillStyle = g; ctx.fillRect(0, y, v.w, 7);
    ctx.strokeStyle = line; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(0, y - 1); ctx.lineTo(v.w, y - 1); ctx.stroke();
  }
  ctx.restore();
}

// Is the pointer on a freeze divider (draggable to change or remove the freeze)?
// Only in the body region (col divider below the column header, row divider
// right of the row header), so it never conflicts with header-border resize.
function freezeHit(px, py) {
  const F = state.freeze;
  if (F.fc > 0 && py > HH && Math.abs(px - F.bodyX0) <= FREEZE_GRAB) return { axis: "col" };
  if (F.fr > 0 && px > HW && Math.abs(py - F.bodyY0) <= FREEZE_GRAB) return { axis: "row" };
  return null;
}

// Commit a freeze-divider drag: the new frozen count is the line/column under
// the pointer; dragging into the header (px<=HW / py<=HH) removes that axis.
function commitFreezeDrag(axis, px, py) {
  const F = state.freeze;
  let fr = F.fr, fc = F.fc;
  if (axis === "col") {
    fc = px <= HW + 2 ? 0 : Math.max(0, colAtX(px));
  } else {
    fr = py <= HH + 2 ? 0 : Math.max(0, rowAtY(py));
  }
  try { wasm.session_set_freeze(state.sheet, fr, fc); } catch (e) { status.textContent = `error: ${e}`; }
  status.textContent = (fc || fr) ? "freeze updated" : "unfrozen";
}

// Show Sum/Avg/Count of the selection (only for a multi-cell selection), like
// a real spreadsheet's status bar.
function fmtNum(n) {
  return Number.isFinite(n) ? (Math.round(n * 1e6) / 1e6).toLocaleString() : String(n);
}
function updateStats() {
  if (!wasm) return;
  const rs = allRanges();
  const single = rs.length === 1;
  const s = rs[0];
  if (single && s.r0 === s.r1 && s.c0 === s.c1) { selStats.textContent = ""; return; }
  // Fold the per-range stats together (disjoint Ctrl+click ranges; overlaps,
  // which Excel also double-counts, are rare).
  let sum = 0, numeric = 0, count = 0;
  let min = Infinity, max = -Infinity;
  for (const r of rs) {
    const st = JSON.parse(wasm.session_range_stats(state.sheet, r.r0, r.c0, r.r1, r.c1));
    sum += st.sum || 0; numeric += st.numeric || 0; count += st.count || 0;
    // The engine has always computed these; the bar just never showed them.
    if (st.numeric) {
      if (st.min !== undefined) min = Math.min(min, st.min);
      if (st.max !== undefined) max = Math.max(max, st.max);
    }
  }
  const parts = [];
  if (numeric > 0) {
    parts.push(`Sum: <b>${fmtNum(sum)}</b>`);
    parts.push(`Avg: <b>${fmtNum(sum / numeric)}</b>`);
    if (Number.isFinite(min)) parts.push(`Min: <b>${fmtNum(min)}</b>`);
    if (Number.isFinite(max)) parts.push(`Max: <b>${fmtNum(max)}</b>`);
    // Excel distinguishes "how many cells have anything" from "how many are
    // numbers"; with both shown, a stray text cell in a numeric column is
    // visible instead of quietly skewing the average.
    if (numeric !== count) parts.push(`Numbers: <b>${numeric}</b>`);
  }
  parts.push(`Count: <b>${count}</b>`);
  selStats.innerHTML = parts.join("&nbsp;&nbsp;&nbsp;");
}

function refreshFormulaBar() {
  if (state.editing) return;
  // Don't clobber a control the user is actively typing in. Background redraws
  // (e.g. the marching-ants copy animation) call this every frame; without this
  // guard they'd reset the formula bar / font / size boxes mid-keystroke.
  const active = document.activeElement;
  if (active === fInput || active === document.getElementById("tb-font") ||
      active === document.getElementById("tb-size")) return;
  fInput.value = wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  document.getElementById("tb-undo").disabled = !wasm.session_can_undo();
  document.getElementById("tb-redo").disabled = !wasm.session_can_redo();
  // Reflect formatting from the selection's top-left (the representative/active
  // cell). For a range/row/column selection state.sel is the *moving end*, which
  // is often an empty corner — reading that left the font/size boxes blank.
  const pr = selRect();
  const fmt = JSON.parse(wasm.session_cell_format(state.sheet, pr.r0, pr.c0));
  const press = (id, on) => document.getElementById(id).setAttribute("aria-pressed", on ? "true" : "false");
  press("tb-bold", fmt.b);
  press("tb-italic", fmt.i);
  press("tb-underline", fmt.u);
  press("tb-strike", fmt.st);
  press("tb-wrap", fmt.w || fmt.cl);
  for (const b of document.querySelectorAll(".tb-align")) {
    b.setAttribute("aria-pressed", b.dataset.al === fmt.al ? "true" : "false");
  }
  document.getElementById("tb-font").value = fmt.fn || "";
  document.getElementById("tb-size").value = fmt.fs ? String(fmt.fs) : "";
}

function cellAt(px, py) {
  if (px < HW || py < HH) return null;
  return { row: rowAtY(py), col: colAtX(px) };
}

function select(row, col) {
  let r = Math.max(0, row);
  let c = Math.max(0, col);
  const m = mergeAt(r, c); // clicking a merged cell selects its anchor (top-left)
  if (m) { r = m.r0; c = m.c0; }
  state.sel = { row: r, col: c };
  state.anchor = { row: r, col: c };
  state.selKind = "cells";
  state.ranges = [];
  ensureVisible();
  draw();
}

// Ctrl/Cmd+click: bank the current range and start a fresh active range at
// (row, col) without clearing the banked ones — builds a multi-range selection.
function addRange(row, col) {
  state.ranges = state.ranges.concat([effectiveRange()]);
  let r = Math.max(0, row), c = Math.max(0, col);
  const m = mergeAt(r, c);
  if (m) { r = m.r0; c = m.c0; }
  state.sel = { row: r, col: c };
  state.anchor = { row: r, col: c };
  state.selKind = "cells";
  draw();
}
// Ctrl/Cmd+click a column/row header: same idea as addRange, but the fresh
// active range is a whole column/row instead of a single cell.
function addColumnRange(c) {
  state.ranges = state.ranges.concat([effectiveRange()]);
  state.selKind = "cols";
  state.anchor = { row: 0, col: c };
  state.sel = { row: 0, col: c };
  endInline();
  draw();
}
function addRowRange(r) {
  state.ranges = state.ranges.concat([effectiveRange()]);
  state.selKind = "rows";
  state.anchor = { row: r, col: 0 };
  state.sel = { row: r, col: 0 };
  endInline();
  draw();
}

// Extend the selection to (row, col), keeping the anchor.
function extend(row, col) {
  state.sel = { row: Math.max(0, row), col: Math.max(0, col) };
  state.selKind = "cells";
  ensureVisible();
  draw();
}

// Scroll the body just enough to show a cell — the active one by default, or
// any cell (arrow-key point mode follows the cell it is pointing at, which is
// not the selection).
function ensureVisible(row = state.sel.row, col = state.sel.col) {
  if (!wasm) return;
  const rect = wrap.getBoundingClientRect();
  const f = state.freeze || { fc: 0, fr: 0, bodyX0: HW, bodyY0: HH };
  // The scrolling viewport is what remains right of / below the frozen bands.
  const viewW = rect.width - f.bodyX0;
  const viewH = rect.height - f.bodyY0;
  const frozenW = fzOffset(col, true, f.fc);
  const frozenH = fzOffset(row, false, f.fr);
  // Frozen cells are always visible; only scroll for cells in the body region.
  if (col >= f.fc) {
    const cL = wasm.session_col_offset_px(state.sheet, col) - frozenW;
    const cW = JSON.parse(wasm.session_col_px(state.sheet, col, 1))[0] || COL_W;
    if (cL < state.scrollX) state.scrollX = cL;
    else if (cL + cW > state.scrollX + viewW) state.scrollX = cL + cW - viewW;
  }
  if (row >= f.fr) {
    const rT = rowOffsetPx(row) - frozenH;
    const rH = JSON.parse(wasm.session_row_px(state.sheet, row, 1))[0] || ROW_H;
    if (rT < state.scrollY) state.scrollY = rT;
    else if (rT + rH > state.scrollY + viewH) state.scrollY = rT + rH - viewH;
  }
  state.scrollX = Math.max(0, state.scrollX);
  state.scrollY = Math.max(0, state.scrollY);
}
// --- Edge auto-scroll while drag-selecting --------------------------------
// When the pointer is dragged into the 28px band at a viewport edge (or past
// it), scroll the body toward the pointer and keep extending the selection —
// like every real spreadsheet. Runs on rAF until the pointer leaves the band
// or the drag ends.
const AUTOSCROLL_EDGE = 28;
function edgeVelocity() {
  if (!dragPos) return { sx: 0, sy: 0, cx: 0, cy: 0 };
  const r = canvas.getBoundingClientRect();
  const f = state.freeze || { bodyX0: HW, bodyY0: HH };
  const { px, py } = dragPos;
  const lo_x = f.bodyX0 + AUTOSCROLL_EDGE, hi_x = r.width - AUTOSCROLL_EDGE;
  const lo_y = f.bodyY0 + AUTOSCROLL_EDGE, hi_y = r.height - AUTOSCROLL_EDGE;
  let sx = 0, sy = 0;
  // A column-header drag only extends columns, so it never auto-scrolls
  // vertically (and vice versa for a row-header drag) — otherwise crossing
  // the top/bottom edge while picking columns would scroll rows too.
  if (state.headerDrag !== "row") {
    if (px > hi_x) sx = px - hi_x; else if (px < lo_x) sx = px - lo_x;
  }
  if (state.headerDrag !== "col") {
    if (py > hi_y) sy = py - hi_y; else if (py < lo_y) sy = py - lo_y;
  }
  // The cell to extend to: pointer clamped into the body region.
  const cx = Math.min(Math.max(px, f.bodyX0 + 1), r.width - 2);
  const cy = Math.min(Math.max(py, f.bodyY0 + 1), r.height - 2);
  return { sx, sy, cx, cy };
}
function autoScrollTick() {
  autoRaf = 0;
  if (!state.dragging || !dragPos) return;
  const { sx, sy, cx, cy } = edgeVelocity();
  if (sx === 0 && sy === 0) return; // pointer back inside — stop
  const SPEED = 0.5;
  const x0 = state.scrollX, y0 = state.scrollY;
  state.scrollX = Math.max(0, state.scrollX + sx * SPEED);
  state.scrollY = Math.max(0, state.scrollY + sy * SPEED);
  // If the scroll is pinned at 0 against the edge (velocity points off-sheet
  // but nothing can move), idle instead of redrawing every frame forever.
  if (state.scrollX === x0 && state.scrollY === y0) return;
  if (state.headerDrag === "col") selectColumn(colAtX(cx), true);
  else if (state.headerDrag === "row") selectRow(rowAtY(cy), true);
  else {
    const hit = cellAt(cx, cy);
    if (hit) { state.sel = { row: hit.row, col: hit.col }; state.selKind = "cells"; }
  }
  draw();
  autoRaf = requestAnimationFrame(autoScrollTick);
}
function maybeAutoScroll() {
  const { sx, sy } = edgeVelocity();
  if (state.dragging && dragPos && (sx !== 0 || sy !== 0)) {
    if (!autoRaf) autoRaf = requestAnimationFrame(autoScrollTick);
  } else {
    stopAutoScroll();
  }
}
function stopAutoScroll() { if (autoRaf) { cancelAnimationFrame(autoRaf); autoRaf = 0; } }

// Absolute px of the frozen band before `count` lines on this axis.
function fzOffset(_line, columns, count) {
  if (count <= 0) return 0;
  return columns
    ? wasm.session_col_offset_px(state.sheet, count)
    : rowOffsetPx(count);
}

// Turn an engine parse error ("[OC-FML-0001] unexpected token: Star") into a
// message a spreadsheet user can act on.
function friendlyFormulaError(err) {
  const m = err.replace(/^\[OC-[A-Z0-9-]+\]\s*/, ""); // drop the internal code
  if (/end of input/i.test(m)) return "the formula looks incomplete — check for a missing value or a closing ‘)’.";
  const t = m.match(/unexpected token:\s*(.+)$/i);
  if (t) {
    const sym = { Star: "*", Plus: "+", Minus: "-", Slash: "/", Caret: "^", RParen: "‘)’", LParen: "‘(’", Comma: "‘,’", Percent: "‘%’" }[t[1].trim()] || t[1].trim();
    return `unexpected ${sym} — check the formula syntax.`;
  }
  return m;
}
function commit(value, advance) {
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
  if (!value.trim().startsWith("=")) {
    let bad = "";
    try { bad = wasm.session_validation_error(state.sheet, state.sel.row, state.sel.col, value); }
    catch {}
    if (bad) {
      status.innerHTML = `<span class="err">Not allowed here — ${bad}</span>`;
      if (editSurface) { editSurface.classList.add("invalid"); editSurface.focus(); }
      return false;
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
      status.innerHTML = `<span class="err">Formula error: ${friendlyFormulaError(err)}</span>`;
      if (editSurface) { editSurface.classList.add("invalid"); editSurface.focus(); }
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
    status.textContent = "ok";
  } catch (e) {
    status.textContent = `error: ${e}`;
  }
  endEdit();
  // Move to the next row on Enter as a fresh single-cell selection (reset the
  // anchor + clear any multi-range, else anchor stays put and paints a ghost
  // 2-cell range).
  if (advance) select(state.sel.row + 1, state.sel.col);
  else { ensureVisible(); draw(); }
  return true;
}

function usedBounds() {
  const b = JSON.parse(wasm.session_used_bounds(state.sheet));
  return { rows: Math.max(1, b.rows), cols: Math.max(1, b.cols) };
}
// Hidden-region markers: a run of zero-width lines is a hidden band. Draw a
// small accent double-bar at each gap in the header strips, and remember the
// spans so a double-click on a marker can unhide them.
let hiddenColMarks = [];
let hiddenRowMarks = [];
function drawHiddenMarkers() {
  hiddenColMarks = [];
  hiddenRowMarks = [];
  let run = null;
  for (let i = 0; i < geo.colIdx.length; i++) {
    if (geo.colW[i] <= 0) {
      if (!run) run = { x: geo.colX[i], from: geo.colIdx[i], to: geo.colIdx[i] };
      else run.to = geo.colIdx[i];
    } else if (run) { hiddenColMarks.push(run); run = null; }
  }
  if (run) hiddenColMarks.push(run);
  run = null;
  for (let i = 0; i < geo.rowIdx.length; i++) {
    if (geo.rowH[i] <= 0) {
      if (!run) run = { y: geo.rowY[i], from: geo.rowIdx[i], to: geo.rowIdx[i] };
      else run.to = geo.rowIdx[i];
    } else if (run) { hiddenRowMarks.push(run); run = null; }
  }
  if (run) hiddenRowMarks.push(run);

  // A bare double-bar marked the spot but read as a freeze line or a selection
  // edge, and it only responded to a double-click — so a hidden row was easy to
  // notice and hard to get back. Draw an actual control: an accent handle with
  // two chevrons pointing apart, the same "expand from here" idiom Sheets uses.
  // Single click unhides (see the mousedown handler); hovering says so.
  ctx.save();
  ctx.fillStyle = colors.accent;
  for (const m of hiddenColMarks) {
    if (m.x < HW) continue;
    const h = Math.min(14, HH - 6);
    const top = (HH - h) / 2;
    ctx.fillRect(m.x - 4, top, 8, h);
    ctx.fillStyle = "#fff";
    const cy = top + h / 2;
    // ‹ ›
    ctx.beginPath();
    ctx.moveTo(m.x - 1.2, cy - 3); ctx.lineTo(m.x - 3.2, cy); ctx.lineTo(m.x - 1.2, cy + 3);
    ctx.closePath();
    ctx.moveTo(m.x + 1.2, cy - 3); ctx.lineTo(m.x + 3.2, cy); ctx.lineTo(m.x + 1.2, cy + 3);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = colors.accent;
  }
  for (const m of hiddenRowMarks) {
    if (m.y < HH) continue;
    const w = Math.min(18, HW - 6);
    const left = (HW - w) / 2;
    ctx.fillRect(left, m.y - 4, w, 8);
    ctx.fillStyle = "#fff";
    const cx = left + w / 2;
    // Chevrons pointing up and down, away from the collapsed band.
    ctx.beginPath();
    ctx.moveTo(cx - 3, m.y - 1.2); ctx.lineTo(cx, m.y - 3.2); ctx.lineTo(cx + 3, m.y - 1.2);
    ctx.closePath();
    ctx.moveTo(cx - 3, m.y + 1.2); ctx.lineTo(cx, m.y + 3.2); ctx.lineTo(cx + 3, m.y + 1.2);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = colors.accent;
  }
  ctx.restore();
}

// The hidden-band handle under a canvas point, if any. Shared by the click
// handler and the hover cursor so the two can never disagree about the target.
function hiddenMarkAt(px, py) {
  if (py < HH && px >= HW) {
    const m = hiddenColMarks.find((g) => g.x >= HW && Math.abs(px - g.x) <= 5);
    if (m) return { axis: "col", mark: m };
  }
  if (px < HW && py >= HH) {
    const m = hiddenRowMarks.find((g) => g.y >= HH && Math.abs(py - g.y) <= 5);
    if (m) return { axis: "row", mark: m };
  }
  return null;
}

// Reveal the band a handle stands for.
function unhideMark(hit) {
  const { from, to } = hit.mark;
  const n = to - from + 1;
  tryEdit(() =>
    hit.axis === "col"
      ? wasm.session_unhide_cols(state.sheet, from, to)
      : wasm.session_unhide_rows(state.sheet, from, to),
  );
  status.textContent = `showed ${n} hidden ${hit.axis === "col" ? "column" : "row"}${n === 1 ? "" : "s"}`;
}

// The outline gutter: a rail per nesting level with a collapse toggle at each
// group's summary line. Rows get the strip left of the row headers, columns the
// one above the column headers; both are zero-width when the axis has no groups.
function drawOutlineGutter(v) {
  outlineToggles = [];
  if (!wasm || (!GW && !GH)) return;
  ctx.save();
  ctx.font = "10px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";

  // One toggle: a bordered box holding − when the group is open, + when shut.
  const toggle = (bx, by, size, collapsed) => {
    ctx.fillStyle = colors.bg;
    ctx.fillRect(bx, by, size, size);
    ctx.strokeStyle = colors.muted;
    ctx.lineWidth = 1;
    ctx.strokeRect(bx + 0.5, by + 0.5, size - 1, size - 1);
    ctx.strokeStyle = colors.fg;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(bx + 2.5, by + size / 2);
    ctx.lineTo(bx + size - 2.5, by + size / 2);
    if (collapsed) {
      ctx.moveTo(bx + size / 2, by + 2.5);
      ctx.lineTo(bx + size / 2, by + size - 2.5);
    }
    ctx.stroke();
  };

  if (GW) {
    const info = readOutline(state.firstRow, geo.rows, false);
    ctx.beginPath();
    ctx.rect(0, HH, GW, v.h - HH);
    ctx.clip();
    for (let i = 0; i < geo.rows; i++) {
      if (geo.rowH[i] <= 0) continue;
      const r = geo.rowIdx[i];
      const line = info[r];
      if (!line) continue;
      const depth = Math.max(0, line.l - (line.b ? 1 : 0));
      const cx = 4 + depth * OUTLINE_STEP;
      // A rail along the lines inside a group, so the extent is visible.
      if (line.l > 0) {
        ctx.fillStyle = colors.muted;
        ctx.globalAlpha = 0.45;
        ctx.fillRect(4 + (line.l - 1) * OUTLINE_STEP + 4, geo.rowY[i], 1, geo.rowH[i]);
        ctx.globalAlpha = 1;
      }
      if (!line.b) continue;
      const size = 9;
      const by = geo.rowY[i] + (geo.rowH[i] - size) / 2;
      toggle(cx, by, size, line.c);
      outlineToggles.push({ x: cx, y: by, w: size, h: size, index: r, columns: false });
    }
  }
  ctx.restore();

  if (GH) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(HW, 0, v.w - HW, GH);
    ctx.clip();
    const info = readOutline(state.firstCol, geo.cols, true);
    for (let i = 0; i < geo.cols; i++) {
      if (geo.colW[i] <= 0) continue;
      const c = geo.colIdx[i];
      const line = info[c];
      if (!line) continue;
      const depth = Math.max(0, line.l - (line.b ? 1 : 0));
      const cy = 4 + depth * OUTLINE_STEP;
      if (line.l > 0) {
        ctx.fillStyle = colors.muted;
        ctx.globalAlpha = 0.45;
        ctx.fillRect(geo.colX[i], 4 + (line.l - 1) * OUTLINE_STEP + 4, geo.colW[i], 1);
        ctx.globalAlpha = 1;
      }
      if (!line.b) continue;
      const size = 9;
      const bx = geo.colX[i] + (geo.colW[i] - size) / 2;
      toggle(bx, cy, size, line.c);
      outlineToggles.push({ x: bx, y: cy, w: size, h: size, index: c, columns: true });
    }
    ctx.restore();
  }
}

// Outline data for the visible window, keyed by line index. Frozen panes mean
// the window is not one contiguous run, so this covers from line 0 when a
// freeze is in play rather than trying to stitch two spans together.
function readOutline(first, count, columns) {
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

// The outline toggle under a canvas point, if any.
function outlineToggleAt(px, py) {
  return outlineToggles.find(
    (t) => px >= t.x && px <= t.x + t.w && py >= t.y && py <= t.y + t.h,
  );
}

// Draw one line stretched to `width` by widening the gaps between words, the
// way horizontal justify/distribute lay out. A line with nothing to stretch (one
// word, or wider than the space already) is drawn plainly rather than having its
// glyphs pulled apart.
function drawStretched(line, x0, width, y) {
  const words = String(line).split(/\s+/).filter(Boolean);
  const prev = ctx.textAlign;
  ctx.textAlign = "left";
  if (words.length < 2) {
    ctx.fillText(String(line), x0, y);
    ctx.textAlign = prev;
    return;
  }
  const ink = words.reduce((sum, wd) => sum + ctx.measureText(wd).width, 0);
  const gap = (width - ink) / (words.length - 1);
  if (gap <= 0) {
    ctx.fillText(String(line), x0, y);
    ctx.textAlign = prev;
    return;
  }
  let wx = x0;
  for (const wd of words) {
    ctx.fillText(wd, wx, y);
    wx += ctx.measureText(wd).width + gap;
  }
  ctx.textAlign = prev;
}

// The column/row index at a canvas x/y (for header clicks + hit-testing).
function colAtX(px) {
  for (let i = 0; i < geo.colX.length; i++) if (px < geo.colX[i] + geo.colW[i]) return geo.colIdx[i];
  return geo.colIdx[geo.colIdx.length - 1] ?? state.firstCol;
}
function rowAtY(py) {
  for (let i = 0; i < geo.rowY.length; i++) if (py < geo.rowY[i] + geo.rowH[i]) return geo.rowIdx[i];
  return geo.rowIdx[geo.rowIdx.length - 1] ?? state.firstRow;
}
// Whole-sheet selection (the top-left corner box). The viewport stays put.
function selectAll() {
  state.selKind = "all";
  state.ranges = [];
  state.anchor = { row: state.firstRow, col: state.firstCol };
  state.sel = { row: state.firstRow, col: state.firstCol };
  endInline();
  draw();
}
// Progressive Ctrl+A (Excel): first press selects the used data region, a
// second press (already covering it) selects the whole sheet.
function ctrlA() {
  const b = usedBounds();
  const r = selRect();
  const coversData = state.selKind === "cells" &&
    r.r0 === 0 && r.c0 === 0 && r.r1 === b.rows - 1 && r.c1 === b.cols - 1;
  if (coversData) { selectAll(); return; }
  state.selKind = "cells";
  state.ranges = [];
  state.anchor = { row: 0, col: 0 };
  state.sel = { row: b.rows - 1, col: b.cols - 1 };
  endInline();
  draw();
}
// Whole-row selection; the focus stays at column 0 so the view doesn't jump.
function selectRow(r, exp) {
  state.selKind = "rows";
  if (!exp) { state.anchor = { row: r, col: 0 }; state.ranges = []; }
  state.sel = { row: r, col: 0 };
  endInline();
  draw();
}
// Whole-column selection; the focus stays at row 0.
function selectColumn(c, exp) {
  state.selKind = "cols";
  if (!exp) { state.anchor = { row: 0, col: c }; state.ranges = []; }
  state.sel = { row: 0, col: c };
  endInline();
  draw();
}
// Shift+Space: promote the current selection to its full rows.
function selectRowsSpan() {
  const r = selRect();
  state.selKind = "rows";
  state.anchor = { row: r.r0, col: 0 };
  state.sel = { row: r.r1, col: 0 };
  state.ranges = [];
  endInline();
  draw();
}
// Ctrl+Space: promote the current selection to its full columns.
function selectColsSpan() {
  const r = selRect();
  state.selKind = "cols";
  state.anchor = { row: 0, col: r.c0 };
  state.sel = { row: 0, col: r.c1 };
  state.ranges = [];
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
// Every rectangle in the selection: the committed extra ranges plus the active
// one. Formatting and stats fold over this so a Ctrl+click multi-range behaves
// as one selection.
function allRanges() {
  return state.ranges.concat([effectiveRange()]);
}
// Double-click a column boundary: size the column to its widest cell, measured
// with each cell's real font (family/size/bold/italic) so larger text fits.
function autofitColumn(col) {
  const b = usedBounds();
  const items = JSON.parse(wasm.session_cells(state.sheet, 0, col, b.rows - 1, col));
  let maxw = 24;
  for (const it of items) {
    if (!it.t) continue;
    ctx.font = cellFont(it);
    maxw = Math.max(maxw, ctx.measureText(String(it.t)).width);
  }
  try { wasm.session_set_col_width(state.sheet, col, Math.ceil(maxw) + 14); } catch {}
  draw();
}
// Double-click a row boundary: size the row to its tallest cell, honoring each
// cell's font size, wrap (wrapped to the column width), and explicit newlines.
function autofitRow(row) {
  const b = usedBounds();
  const items = JSON.parse(wasm.session_cells(state.sheet, row, 0, row, b.cols - 1));
  let maxh = ROW_H;
  for (const it of items) {
    if (!it.t) continue;
    // Match measure()'s per-cell math exactly, or autofit and the renderer
    // disagree — and since autofit *persists* the height (which pins the row
    // against further auto-height), a mismatch here is not self-correcting.
    if (it.w) {
      const colW = Math.max(8, colWAt(it.c) - 8);
      const lines = String(it.t).split("\n").flatMap((seg) => wrapLines({ ...it, t: seg }, colW));
      maxh = Math.max(maxh, lines.length * cellLineH(it) + 6);
    } else {
      const lines = String(it.t).split("\n").length;
      maxh = Math.max(maxh, lines === 1 ? cellPx(it) + 5 : lines * cellLineH(it) + 6);
    }
  }
  try { wasm.session_set_row_height(state.sheet, row, Math.ceil(maxh)); } catch {}
  draw();
}

// Run a formatting op over the whole selection (every range), then redraw.
function formatSel(fn) {
  try { for (const s of allRanges()) fn(s); } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}
function toggleBold() { formatSel((s) => wasm.session_toggle_bold(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleItalic() { formatSel((s) => wasm.session_toggle_italic(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleUnderline() { formatSel((s) => wasm.session_toggle_underline(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleStrike() { formatSel((s) => wasm.session_toggle_strike(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function setFill(hex) { formatSel((s) => wasm.session_set_fill(state.sheet, s.r0, s.c0, s.r1, s.c1, hex)); }
function setFontColor(hex) { formatSel((s) => wasm.session_set_font_color(state.sheet, s.r0, s.c0, s.r1, s.c1, hex)); }

// A standard color palette (grays row + hue columns at 5 lightness levels),
// shared by the font- and fill-color popovers, plus recent colors and a custom
// hex entry so any RRGGBB the engine supports is reachable.
const COLOR_PALETTE = [
  "000000", "434343", "666666", "999999", "b7b7b7", "cccccc", "d9d9d9", "efefef", "f3f3f3", "ffffff",
  "980000", "ff0000", "ff9900", "ffff00", "00ff00", "00ffff", "4a86e8", "0000ff", "9900ff", "ff00ff",
  "e6b8af", "f4cccc", "fce5cd", "fff2cc", "d9ead3", "d0e0e3", "c9daf8", "cfe2f3", "d9d2e9", "ead1dc",
  "dd7e6b", "ea9999", "f9cb9c", "ffe599", "b6d7a8", "a2c4c9", "a4c2f4", "9fc5e8", "b4a7d6", "d5a6bd",
  "cc4125", "e06666", "f6b26b", "ffd966", "93c47d", "76a5af", "6d9eeb", "6fa8dc", "8e7cc3", "c27ba0",
];
let recentColors = [];
function pushRecent(hex) {
  const h = (hex || "").toUpperCase();
  if (!h) return;
  recentColors = [h, ...recentColors.filter((c) => c !== h)].slice(0, 10);
}
// Build a color popover into `menu`; `onPick(hex)` applies ("" clears).
// Manage Rules: the sheet's conditional formats in the order they are actually
// evaluated, with the reorder / stop-if-true / delete controls that order needs
// to be meaningful. Without this, rules could only be added and cleared
// wholesale, and which of two overlapping rules won was invisible.
function manageCfRules() {
  const modal = document.getElementById("oc-modal");
  const body = document.getElementById("oc-modal-body");
  document.getElementById("oc-modal-title").textContent = "Conditional formatting rules";

  const close = () => { modal.hidden = true; body.textContent = ""; };
  const render = () => {
    body.textContent = "";
    let rules = [];
    try { rules = JSON.parse(wasm.session_cf_rules(state.sheet)); } catch {}
    if (!rules.length) {
      body.append(el("p", "oc-confirm-text", "No conditional formatting on this sheet."));
    } else {
      body.append(el("p", "oc-confirm-text", "Listed in evaluation order — the first rule that matches a cell wins."));
      const list = el("div", "cf-rules");
      for (const r of rules) {
        const row = el("div", "cf-rule-row");
        const sw = el("span", "cf-rule-swatch");
        if (r.fill) sw.style.background = "#" + r.fill;
        const label = el("span", "cf-rule-text", `${r.range} — ${r.desc}`);
        const stop = document.createElement("input");
        stop.type = "checkbox";
        stop.checked = !!r.stop;
        stop.title = "Stop evaluating later rules when this one matches";
        stop.addEventListener("change", () => {
          tryEdit(() => wasm.session_set_cf_stop(state.sheet, r.i, stop.checked));
          render();
        });
        const up = el("button", "oc-btn", "↑");
        up.title = "Evaluate earlier";
        up.addEventListener("click", () => { tryEdit(() => wasm.session_reorder_cf_rule(state.sheet, r.i, true)); render(); });
        const down = el("button", "oc-btn", "↓");
        down.title = "Evaluate later";
        down.addEventListener("click", () => { tryEdit(() => wasm.session_reorder_cf_rule(state.sheet, r.i, false)); render(); });
        const del = el("button", "oc-btn", "Delete");
        del.addEventListener("click", () => { tryEdit(() => wasm.session_delete_cf_rule(state.sheet, r.i)); render(); });
        row.append(sw, label, stop, up, down, del);
        list.appendChild(row);
      }
      body.appendChild(list);
    }
    const actions = el("div", "oc-confirm-actions");
    const done = el("button", "oc-btn primary", "Close");
    done.addEventListener("click", () => { close(); canvas.focus(); });
    actions.appendChild(done);
    body.appendChild(actions);
    done.focus();
  };
  render();
  modal.hidden = false;
}

// The named cell-style gallery. Applying one writes its formatting *and*
// records which style the cells belong to, so the association survives a save —
// that link is the whole point of a named style over ad-hoc formatting.
function cellStyleGallery() {
  let styles = [];
  try { styles = JSON.parse(wasm.session_cell_styles()); } catch {}
  if (!styles.length) { status.textContent = "no cell styles available"; return; }

  const modal = document.getElementById("oc-modal");
  const body = document.getElementById("oc-modal-body");
  document.getElementById("oc-modal-title").textContent = "Cell styles";
  body.textContent = "";
  body.append(el("p", "oc-confirm-text", "Applies the style's formatting and tags the cells with its name."));

  const grid = el("div", "style-gallery");
  const close = () => { modal.hidden = true; body.textContent = ""; };
  for (const st of styles) {
    const b = el("button", "style-chip", st.n);
    // Preview each entry in its own look, which is the only way to tell
    // "Heading 2" from "Heading 3" at a glance.
    if (st.bg) b.style.background = "#" + st.bg;
    if (st.fg) b.style.color = "#" + st.fg;
    if (st.bold) b.style.fontWeight = "700";
    if (st.sz) b.style.fontSize = Math.min(18, Math.max(11, st.sz)) + "px";
    b.addEventListener("click", () => {
      close();
      canvas.focus();
      formatSel((r) => wasm.session_apply_cell_style(state.sheet, r.r0, r.c0, r.r1, r.c1, st.n));
      status.textContent = `applied cell style "${st.n}"`;
    });
    grid.appendChild(b);
  }
  body.appendChild(grid);

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Close");
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  actions.appendChild(cancel);
  body.appendChild(actions);
  modal.hidden = false;
  grid.querySelector("button")?.focus();
}

// Parse the colour notations people actually paste: `#abc`, `abc`, `#aabbcc`,
// `aabbcc`, `rgb(1,2,3)` / `rgba(...)`, and `hsl(h,s%,l%)`. Returns `RRGGBB` or
// null. Accepting only 6-digit hex rejected half of what a designer copies.
function parseColor(input) {
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

// Shade a colour toward white (positive tint) or black (negative), the same way
// OOXML's `tint` attribute does — so the theme row's lighter/darker variants are
// the ones the file itself would produce.
function tintColor(hex, tint) {
  const n = parseInt(hex, 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) =>
    tint >= 0 ? c + (255 - c) * tint : c * (1 + tint),
  );
  return ch.map((c) => Math.round(c).toString(16).padStart(2, "0")).join("").toUpperCase();
}

function buildColorMenu(menu, onPick, noneLabel) {
  menu.textContent = "";
  const pick = (hex) => { pushRecent(hex); onPick(hex); menu.hidden = true; canvas.focus(); };
  const none = el("button", "cm-none");
  none.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="icon-sm"><circle cx="12" cy="12" r="9"/><line x1="5.6" y1="5.6" x2="18.4" y2="18.4"/></svg>' +
    `<span>${noneLabel}</span>`;
  none.addEventListener("click", (e) => { e.stopPropagation(); pick(""); });
  menu.appendChild(none);

  const grid = (colors) => {
    const g = el("div", "cm-grid");
    for (const c of colors) {
      const b = el("button", "cm-sw");
      b.style.background = "#" + c;
      b.title = "#" + c;
      b.addEventListener("click", (e) => { e.stopPropagation(); pick(c); });
      g.appendChild(b);
    }
    return g;
  };
  if (recentColors.length) {
    menu.appendChild(el("div", "cm-label", "Recent"));
    menu.appendChild(grid(recentColors));
  }
  // The workbook's own theme, not a stock imitation of one: the engine hands
  // back the slots it read from `theme1.xml`. Slot order is OOXML's, and the
  // first four are the light/dark background/text pairs — shown in the order
  // Excel shows them so the swatches sit where people expect.
  let theme = [];
  try { theme = JSON.parse(wasm.theme_colors()); } catch {}
  if (theme.length >= 10) {
    const order = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    const base = order.map((i) => theme[i]).filter(Boolean);
    menu.appendChild(el("div", "cm-label", "Theme"));
    menu.appendChild(grid(base));
    // Excel's tint ladder under the base row: lighter above, darker below.
    for (const t of [0.6, 0.4, -0.25, -0.5]) {
      menu.appendChild(grid(base.map((c) => tintColor(c, t))));
    }
  }
  menu.appendChild(el("div", "cm-label", "Standard"));
  menu.appendChild(grid(COLOR_PALETTE));

  menu.appendChild(el("div", "cm-label", "Custom"));
  const custom = el("div", "cm-custom");
  const hex = el("input", "cm-hex");
  hex.placeholder = "#RRGGBB";
  hex.spellcheck = false;
  hex.addEventListener("click", (e) => e.stopPropagation());
  const apply = el("button", "cm-apply", "Apply");
  const commitHex = () => {
    const parsed = parseColor(hex.value);
    if (parsed) pick(parsed);
    else { hex.style.borderColor = "#e5484d"; }
  };
  hex.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.stopPropagation(); commitHex(); } });
  hex.addEventListener("input", () => { hex.style.borderColor = ""; });
  apply.addEventListener("click", (e) => { e.stopPropagation(); commitHex(); });
  custom.appendChild(hex);
  custom.appendChild(apply);
  menu.appendChild(custom);

  // Native colour dialog — the full HS/V surface, without shipping one.
  const more = el("div", "cm-custom");
  const native = el("input", "cm-native");
  native.type = "color";
  native.title = "More colours";
  native.addEventListener("click", (e) => e.stopPropagation());
  native.addEventListener("change", (e) => { e.stopPropagation(); pick(native.value.replace("#", "").toUpperCase()); });
  more.appendChild(native);
  // Eyedropper is Chromium-only, so it appears only where it works rather than
  // sitting there dead.
  if (window.EyeDropper) {
    const drop = el("button", "cm-apply", "Pick from screen");
    drop.addEventListener("click", async (e) => {
      e.stopPropagation();
      try {
        const { sRGBHex } = await new window.EyeDropper().open();
        const parsed = parseColor(sRGBHex);
        if (parsed) pick(parsed);
      } catch {
        // The user dismissed the picker; nothing to report.
      }
    });
    more.appendChild(drop);
  }
  menu.appendChild(more);
}
function setAlign(al) { formatSel((s) => wasm.session_set_align(state.sheet, s.r0, s.c0, s.r1, s.c1, al)); }
function setValign(va) { formatSel((s) => wasm.session_set_valign(state.sheet, s.r0, s.c0, s.r1, s.c1, va)); }
function toggleWrap() { formatSel((s) => wasm.session_toggle_wrap(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
// One three-way choice: "overflow" (spill into empty neighbours), "wrap", or
// "clip" (stop at the cell edge). Wrap and clip are mutually exclusive, so the
// engine sets them together rather than exposing two toggles that can disagree.
// --- Format painter ---------------------------------------------------------
// Pick up the active cell's formatting, then apply it to the next selection.
// A single click paints once and disarms; a double-click stays armed so a
// format can be brushed onto several places, as in Excel. Escape cancels.
let painter = null; // { row, col, sticky }
function setPainter(next) {
  painter = next;
  const btn = document.getElementById("tb-painter");
  if (btn) btn.setAttribute("aria-pressed", next ? "true" : "false");
  canvas.style.cursor = next ? "copy" : "cell";
}
function armPainter(sticky) {
  setPainter({ row: state.sel.row, col: state.sel.col, sticky });
  status.textContent = sticky
    ? "format painter: select cells to paint (Esc to stop)"
    : "format painter: select cells to paint";
}
// Apply the picked-up format to a range. Returns whether it painted, so the
// caller can decide whether the click was consumed.
function applyPainter(s) {
  if (!painter) return false;
  const { row, col, sticky } = painter;
  try {
    wasm.session_copy_style(state.sheet, row, col, s.r0, s.c0, s.r1, s.c1);
    status.textContent = "format applied";
  } catch (e) { status.textContent = `error: ${e}`; }
  if (!sticky) setPainter(null);
  draw();
  return true;
}

function setRotation(rot) {
  formatSel((s) => wasm.session_set_rotation(state.sheet, s.r0, s.c0, s.r1, s.c1, rot));
}

function setIndent(delta) {
  formatSel((s) => wasm.session_adjust_indent(state.sheet, s.r0, s.c0, s.r1, s.c1, delta));
}

function setTextOverflow(mode) {
  formatSel((s) => wasm.session_set_text_overflow(state.sheet, s.r0, s.c0, s.r1, s.c1, mode));
}
function setFreeze(kind) {
  if (kind === "gridlines") {
    try { wasm.session_set_gridlines_hidden(state.sheet, !wasm.session_gridlines_hidden(state.sheet)); }
    catch (e) { status.textContent = `error: ${e}`; }
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
  } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}
// Sort the current selection's rows by the active cell's column. A single-cell
// selection sorts the whole used data region (rows 1..end, keeping a header row
// out only if the caller selected a body range). Ascending unless `desc`.
// The block a sort should act on: the selection, or — for a lone cell — the
// whole used area, which is Excel's "sort this column" gesture.
function sortTarget() {
  const s = effectiveRange();
  if (s.r0 === s.r1 && s.c0 === s.c1) {
    const b = usedBounds();
    return { r0: 0, c0: 0, r1: b.rows - 1, c1: b.cols - 1 };
  }
  return s;
}

// Whether a range's first row looks like column headings, by Excel's rule of
// thumb: a heading is text sitting over data that is not. Getting this wrong in
// either direction is destructive — sorting the heading into the middle of the
// data, or treating the first record as a heading and leaving it behind — so
// the answer is only ever a *default* for the dialog's checkbox.
function looksLikeHeader(s) {
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

function sortRange(desc) {
  const s = sortTarget();
  const hasHeader = looksLikeHeader(s);
  const key = Math.min(Math.max(state.sel.col, s.c0), s.c1);
  applySort(s, [{ col: key, asc: !desc }], hasHeader);
}

// Run a sort, excluding the heading row when there is one.
function applySort(s, keys, hasHeader) {
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
  } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}

// The custom number-format dialog. The engine understands far more codes than
// the preset menu offers — scientific, section colours, a text section — and
// without somewhere to type one, none of that is reachable. Previews against
// the active cell's own value, so you can see what the code does to *your*
// data before applying it.
function customFormatDialog() {
  const modal = document.getElementById("oc-modal");
  const body = document.getElementById("oc-modal-body");
  document.getElementById("oc-modal-title").textContent = "Custom number format";
  body.textContent = "";

  let current = "";
  let sample = 1234.567;
  let sampleText = "";
  try {
    const fmt = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col));
    current = fmt.nf || "";
    const it = JSON.parse(wasm.session_cells(state.sheet, state.sel.row, state.sel.col, state.sel.row, state.sel.col))[0];
    if (it && it.n) sample = parseFloat(wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col)) || sample;
    else if (it && it.t) sampleText = it.t;
  } catch {}

  const input = document.createElement("input");
  input.type = "text";
  input.className = "cf-code";
  input.spellcheck = false;
  input.value = current;
  input.placeholder = "#,##0.00;[Red]-#,##0.00";
  const preview = el("div", "cf-preview");
  const hint = el("p", "cf-hint",
    "Sections are separated by ; — positive;negative;zero;text. [Red] colours a section, @ stands for the text value.");

  const render = () => {
    const code = input.value.trim();
    try {
      preview.textContent = code
        ? (sampleText
            ? wasm.format_preview_text(sampleText, code)
            : wasm.format_preview(sample, code))
        : String(sampleText || sample);
      preview.classList.remove("bad");
    } catch {
      preview.textContent = "—";
      preview.classList.add("bad");
    }
  };
  input.addEventListener("input", render);
  render();

  const presets = el("div", "cf-presets");
  for (const [label, code] of [
    ["Red negatives", "#,##0.00;[Red]-#,##0.00"],
    ["Thousands", "#,##0"],
    ["Scientific", "0.00E+00"],
    ["Accounting-ish", "$#,##0.00;[Red]($#,##0.00);\"-\""],
    ["Text", "@"],
    ["Suffix", "0\" kg\""],
  ]) {
    const b = el("button", "cf-preset", label);
    b.title = code;
    b.addEventListener("click", () => { input.value = code; render(); input.focus(); });
    presets.appendChild(b);
  }

  // Currency builder. A currency format is `[$SYM-locale]`, and the locale id is
  // a hex LCID nobody remembers — so the code is assembled rather than typed.
  // The symbol goes *inside* the bracket: writing a bare "£" would work until it
  // met a format that treats the character as a literal in the wrong section.
  const CURRENCIES = [
    ["$", "409", "US dollar"],
    ["£", "809", "Pound sterling"],
    ["€", "407", "Euro"],
    ["¥", "411", "Japanese yen"],
    ["₹", "4009", "Indian rupee"],
    ["CHF", "807", "Swiss franc"],
    ["A$", "C09", "Australian dollar"],
    ["C$", "1009", "Canadian dollar"],
  ];
  const curWrap = el("div", "cf-currency");
  const curSel = document.createElement("select");
  curSel.className = "panel-select";
  curSel.setAttribute("aria-label", "Currency");
  for (const [sym, lcid, name] of CURRENCIES) {
    const o = el("option", null, `${sym} — ${name}`);
    o.value = `${sym}|${lcid}`;
    curSel.appendChild(o);
  }
  const decSel = document.createElement("select");
  decSel.className = "panel-select";
  decSel.setAttribute("aria-label", "Decimal places");
  for (const d of [0, 2]) {
    const o = el("option", null, d === 0 ? "no decimals" : "2 decimals");
    o.value = String(d);
    decSel.appendChild(o);
  }
  decSel.value = "2";
  const redNeg = document.createElement("input");
  redNeg.type = "checkbox";
  const redLabel = el("label", "cf-redneg");
  redLabel.append(redNeg, document.createTextNode(" red negatives"));
  const build = el("button", "cf-preset", "Insert currency format");
  build.addEventListener("click", () => {
    const [sym, lcid] = curSel.value.split("|");
    const dp = decSel.value === "0" ? "" : ".00";
    const money = `[$${sym}-${lcid}]#,##0${dp}`;
    input.value = redNeg.checked ? `${money};[Red]-${money}` : money;
    render();
    input.focus();
  });
  curWrap.append(curSel, decSel, redLabel, build);

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Apply");
  actions.append(cancel, ok);
  body.append(el("p", "oc-confirm-text", "Format code"), input, preview,
              el("p", "oc-confirm-text", "Currency"), curWrap, presets, hint, actions);
  modal.hidden = false;
  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", close);
  ok.addEventListener("click", () => { const code = input.value.trim(); close(); canvas.focus(); setNumberFormat(code); });
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") ok.click();
    else if (e.key === "Escape") { close(); canvas.focus(); }
  });
  input.focus();
  input.select();
}

// Remove duplicate rows from the selection (or the used area for a lone cell),
// keeping the first of each. Asks first, since it deletes rows, and reports how
// many went — "removed 0" is a useful answer too.
async function removeDuplicates() {
  const s = sortTarget();
  const hasHeader = looksLikeHeader(s);
  const first = hasHeader ? s.r0 + 1 : s.r0;
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
  } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}

// The Sort dialog: choose up to three keys and say whether row 1 is a heading.
// The single-click A→Z / Z→A menu items stay for the common case; this is for
// when "sort by region, then by total descending" is what you actually meant.
function sortDialog() {
  const s = sortTarget();
  const modal = document.getElementById("oc-modal");
  const body = document.getElementById("oc-modal-body");
  document.getElementById("oc-modal-title").textContent = "Sort range";
  body.textContent = "";

  const where = el("p", "oc-confirm-text",
    `${colName(s.c0)}${s.r0 + 1}:${colName(s.c1)}${s.r1 + 1} — ${s.r1 - s.r0 + 1} rows`);
  const headerRow = el("label", "sort-head");
  const headerBox = document.createElement("input");
  headerBox.type = "checkbox";
  headerBox.checked = looksLikeHeader(s);
  headerRow.append(headerBox, document.createTextNode(" My data has a header row"));

  const keysWrap = el("div", "sort-keys");
  const cols = [];
  for (let c = s.c0; c <= s.c1; c += 1) cols.push(c);
  const headingOf = (c) => {
    if (!headerBox.checked) return colName(c);
    try {
      const it = JSON.parse(wasm.session_cells(state.sheet, s.r0, c, s.r0, c))[0];
      if (it && it.t) return `${colName(c)} — ${it.t}`;
    } catch {}
    return colName(c);
  };
  const rows = [];
  const addKeyRow = (index) => {
    const row = el("div", "sort-key");
    row.append(el("span", "sort-lbl", index === 0 ? "Sort by" : "Then by"));
    const pick = document.createElement("select");
    const none = document.createElement("option");
    none.value = ""; none.textContent = "—";
    if (index > 0) pick.appendChild(none);
    for (const c of cols) {
      const o = document.createElement("option");
      o.value = String(c);
      o.textContent = headingOf(c);
      pick.appendChild(o);
    }
    pick.value = String(index === 0 ? Math.min(Math.max(state.sel.col, s.c0), s.c1) : "");
    const dir = document.createElement("select");
    for (const [v, t] of [["asc", "A → Z"], ["desc", "Z → A"]]) {
      const o = document.createElement("option");
      o.value = v; o.textContent = t;
      dir.appendChild(o);
    }
    row.append(pick, dir);
    keysWrap.appendChild(row);
    rows.push({ pick, dir });
  };
  [0, 1, 2].forEach(addKeyRow);
  // Re-label the pickers when the header checkbox flips, so they name the
  // columns the way the user now thinks of them.
  headerBox.addEventListener("change", () => {
    for (const { pick } of rows) {
      [...pick.options].forEach((o) => { if (o.value !== "") o.textContent = headingOf(+o.value); });
    }
  });

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Sort");
  actions.append(cancel, ok);
  body.append(where, headerRow, keysWrap, actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", close);
  ok.addEventListener("click", () => {
    const keys = rows
      .filter(({ pick }) => pick.value !== "")
      .map(({ pick, dir }) => ({ col: +pick.value, asc: dir.value === "asc" }));
    close();
    canvas.focus();
    if (keys.length) applySort(s, keys, headerBox.checked);
  });
  ok.focus();
}
// --- Autofilter -----------------------------------------------------------
// Per-column filtering, held by the engine rather than here: the rules live on
// the sheet (so they save to .xlsx and undo as one step) and the rows they hide
// are a set of their own, separate from rows hidden by hand. Clearing a filter
// therefore releases exactly the rows it hid.
let filterInfo = null;    // {r0,c0,r1,c1,cols:Set<absCol>,hidden} or null
let filterButtons = [];   // hit targets rebuilt each frame by drawFilterButtons()

function refreshFilterInfo() {
  filterInfo = null;
  if (!wasm) return;
  try {
    const j = wasm.session_filter_info(state.sheet);
    if (j && j !== "null") {
      filterInfo = JSON.parse(j);
      filterInfo.cols = new Set(filterInfo.cols);
    }
  } catch {}
}

// Turn the filter on over the current block, or off if one is already on.
function toggleFilter() {
  if (!wasm) return;
  if (filterInfo) {
    tryEdit(() => wasm.session_clear_filter(state.sheet));
    status.textContent = "filter removed";
  } else {
    // An explicit multi-cell selection wins; otherwise take the used region,
    // which is what a user pressing the button on a single cell means.
    const r = effectiveRange();
    const multi = r.r1 > r.r0 || r.c1 > r.c0;
    const b = usedBounds();
    if (!multi && (b.rows < 2 || b.cols < 1)) { status.textContent = "nothing to filter"; return; }
    const box = multi ? r : { r0: 0, c0: 0, r1: b.rows - 1, c1: b.cols - 1 };
    tryEdit(() => wasm.session_set_filter_range(state.sheet, box.r0, box.c0, box.r1, box.c1));
    status.textContent = "filter on — click a header button to filter a column";
  }
}

// Draw a dropdown button on each header cell in the filter range, recording the
// hit targets. A column carrying a rule gets the accent treatment so it is
// obvious at a glance which columns are narrowing the view.
//
// `withQuad` is draw()'s pane clipper, passed in because it closes over the
// frame's frozen-pane geometry: a button on a header scrolled under a frozen
// pane must be clipped to its own pane, not painted over the frozen one.
function drawFilterButtons(withQuad) {
  filterButtons = [];
  if (!filterInfo) return;
  const row = filterInfo.r0;
  const y = rowYAt(row);
  if (y === undefined) return;
  const ch = rowHAt(row);
  // Header fills, so the glyph can be drawn in whatever contrasts with the cell
  // it sits on — a header is usually filled, and the fill can be any colour.
  const fillOf = new Map();
  for (const it of geoItems) if (it.r === row && it.bg) fillOf.set(it.c, it.bg);

  for (let col = filterInfo.c0; col <= filterInfo.c1; col++) {
    const x = colXAt(col);
    if (x === undefined) continue;
    const cw = colWAt(col);
    // Skip a column too narrow to hold the glyph without covering its label.
    if (cw < 22) continue;
    const size = 12;
    const bx = Math.round(x + cw - size - 4), by = Math.round(y + (ch - size) / 2);
    const active = filterInfo.cols.has(col);
    const ink = contrastInk(fillOf.get(col));
    withQuad(row, col, () => {
      ctx.save();
      // Following Sheets: an inverted triangle while the column is merely
      // filterable, a funnel once a rule is on it — the shape carries the state,
      // so it survives being small and needs no colour cue.
      //
      // Both are filled. A 1px outline at this size renders muddy once the
      // canvas transform (dpr x zoom) puts its edges off-pixel; a solid shape
      // stays crisp at any scale.
      const mx = bx + size / 2, my = by + size / 2;
      ctx.fillStyle = ink;
      ctx.globalAlpha = active ? 1 : 0.75;
      ctx.beginPath();
      if (active) {
        // Symmetric funnel: flat top, sides converging to a straight stem.
        ctx.moveTo(mx - 5, my - 4.5);
        ctx.lineTo(mx + 5, my - 4.5);
        ctx.lineTo(mx + 1.6, my - 0.6);
        ctx.lineTo(mx + 1.6, my + 4.5);
        ctx.lineTo(mx - 1.6, my + 4.5);
        ctx.lineTo(mx - 1.6, my - 0.6);
      } else {
        ctx.moveTo(mx - 4, my - 2.2);
        ctx.lineTo(mx + 4, my - 2.2);
        ctx.lineTo(mx, my + 2.8);
      }
      ctx.closePath();
      ctx.fill();
      ctx.restore();
    });
    filterButtons.push({ x: bx, y: by, w: size, h: size, col, row });
  }
}

// Ink that reads against a cell fill (`RRGGBB`, no `#`). Uses the sRGB relative
// luminance the WCAG contrast ratio is built on, so a mid-tone fill flips at the
// point where white actually starts winning rather than at a guessed midpoint.
// No fill means the sheet background, so fall back to the theme foreground.
function contrastInk(hex) {
  if (!hex || hex.length < 6) return colors.fg;
  const lin = (c) => {
    const v = parseInt(hex.slice(c, c + 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  const l = 0.2126 * lin(0) + 0.7152 * lin(2) + 0.0722 * lin(4);
  return l > 0.179 ? "#111418" : "#ffffff";
}


// The button under a canvas point, if any.
function filterButtonAt(px, py) {
  return filterButtons.find(
    (b) => px >= b.x && px <= b.x + b.w && py >= b.y && py <= b.y + b.h,
  );
}

// The per-column dropdown: a searchable checklist plus a conditions submenu.
function openColumnFilter(col, x, y) {
  closeSheetMenu();
  let payload;
  try { payload = JSON.parse(wasm.session_filter_values(state.sheet, col)); }
  catch { status.textContent = "could not read column values"; return; }
  const all = payload.values || [];

  // Working set of checked values, seeded from what the engine reports.
  const checked = new Set(all.filter((v) => v.c).map((v) => v.v));

  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu filter-menu";
  menu.id = "sheet-ctx";
  menu.addEventListener("click", (e) => e.stopPropagation());

  const head = document.createElement("div");
  head.className = "menu-label";
  head.textContent = `Filter ${colName(col)}`;
  menu.appendChild(head);

  // Conditions — the entry point to the two-comparison dialog.
  const cond = document.createElement("button");
  cond.className = "menu-item filter-cond";
  cond.textContent = payload.custom ? "Edit condition…" : "Filter by condition…";
  cond.addEventListener("click", () => { closeSheetMenu(); conditionDialog(col); });
  menu.appendChild(cond);

  if (payload.custom) {
    const note = document.createElement("div");
    note.className = "panel-hint";
    note.textContent = "A condition is active on this column. Ticking values below replaces it.";
    menu.appendChild(note);
  }
  if (payload.truncated) {
    const note = document.createElement("div");
    note.className = "panel-hint";
    note.textContent = `Only the first ${all.length} distinct values are listed — use a condition to match the rest.`;
    menu.appendChild(note);
  }

  const search = document.createElement("input");
  search.type = "search";
  search.className = "filter-search";
  search.placeholder = "Search values";
  search.setAttribute("aria-label", `Search values in ${colName(col)}`);
  menu.appendChild(search);

  const allRow = document.createElement("label");
  allRow.className = "filter-item filter-all";
  const allCb = document.createElement("input");
  allCb.type = "checkbox";
  allRow.appendChild(allCb);
  allRow.appendChild(document.createTextNode("(Select all)"));
  menu.appendChild(allRow);

  const list = document.createElement("div");
  list.className = "filter-list";
  menu.appendChild(list);

  // Rebuild the visible rows for the current search text. (Select all) applies
  // to what is *shown*, which is what makes search-then-tick usable.
  let shown = all;
  function build() {
    const q = search.value.trim().toLowerCase();
    shown = q ? all.filter((v) => v.v.toLowerCase().includes(q)) : all;
    list.textContent = "";
    for (const item of shown) {
      const row = document.createElement("label");
      row.className = "filter-item";
      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.checked = checked.has(item.v);
      cb.addEventListener("change", () => {
        if (cb.checked) checked.add(item.v); else checked.delete(item.v);
        syncAll();
      });
      row.appendChild(cb);
      row.appendChild(document.createTextNode(item.v === "" ? "(Blanks)" : item.v));
      list.appendChild(row);
    }
    if (!shown.length) {
      const none = document.createElement("div");
      none.className = "panel-hint";
      none.textContent = "No matching values";
      list.appendChild(none);
    }
    syncAll();
  }
  function syncAll() {
    const on = shown.filter((v) => checked.has(v.v)).length;
    allCb.checked = shown.length > 0 && on === shown.length;
    allCb.indeterminate = on > 0 && on < shown.length;
  }
  allCb.addEventListener("change", () => {
    for (const v of shown) { if (allCb.checked) checked.add(v.v); else checked.delete(v.v); }
    build();
  });
  search.addEventListener("input", build);
  build();

  const foot = document.createElement("div");
  foot.className = "filter-foot";
  const clr = document.createElement("button");
  clr.className = "filter-clear";
  clr.textContent = "Clear";
  clr.addEventListener("click", () => {
    closeSheetMenu();
    tryEdit(() => wasm.session_set_filter_values(state.sheet, col, []));
    afterFilterChange();
  });
  const apply = document.createElement("button");
  apply.className = "filter-apply";
  apply.textContent = "Apply";
  apply.addEventListener("click", () => {
    closeSheetMenu();
    // Everything ticked means "no rule" — same as clearing, and it keeps the
    // saved file free of a filter that excludes nothing.
    const values = all.every((v) => checked.has(v.v)) ? [] : all.filter((v) => checked.has(v.v)).map((v) => v.v);
    if (values.length === 0 && !all.every((v) => checked.has(v.v))) {
      status.textContent = "tick at least one value";
      return;
    }
    tryEdit(() => wasm.session_set_filter_values(state.sheet, col, values));
    afterFilterChange();
  });
  foot.appendChild(clr);
  foot.appendChild(apply);
  menu.appendChild(foot);

  positionMenu(menu, x, y);
  search.focus();
}

// Report what the filter now hides. The repaint already happened inside
// tryEdit, and draw() refreshes `filterInfo`, so this only reads the result.
function afterFilterChange() {
  const n = filterInfo ? filterInfo.hidden : 0;
  status.textContent = n ? `filtered — ${n} row${n === 1 ? "" : "s"} hidden` : "filter cleared";
}

// The two-comparison condition dialog for one column.
function conditionDialog(col) {
  const OPS = [
    ["equal", "equals"],
    ["notEqual", "does not equal"],
    ["greaterThan", "is greater than"],
    ["greaterThanOrEqual", "is greater than or equal to"],
    ["lessThan", "is less than"],
    ["lessThanOrEqual", "is less than or equal to"],
    ["contains", "contains"],
    ["notContains", "does not contain"],
    ["beginsWith", "begins with"],
    ["endsWith", "ends with"],
  ];
  // The last four are not OOXML operators. Excel stores them as equal /
  // notEqual with wildcards, so translate here and keep the written file honest
  // rather than inventing an operator no other reader would understand.
  const encode = (op, val) => {
    switch (op) {
      case "contains": return ["equal", `*${val}*`];
      case "notContains": return ["notEqual", `*${val}*`];
      case "beginsWith": return ["equal", `${val}*`];
      case "endsWith": return ["equal", `*${val}`];
      default: return [op, val];
    }
  };

  const modal = document.getElementById("oc-modal");
  const body = document.getElementById("oc-modal-body");
  document.getElementById("oc-modal-title").textContent = "Filter by condition";
  body.textContent = "";

  const where = el("p", "oc-confirm-text", `Show rows where ${colName(col)}:`);
  const mkRow = (n) => {
    const row = el("div", "filter-cond-row");
    const sel = document.createElement("select");
    sel.setAttribute("aria-label", `Condition ${n} operator`);
    if (n === 2) sel.append(new Option("(none)", ""));
    for (const [v, label] of OPS) sel.append(new Option(label, v));
    const inp = document.createElement("input");
    inp.type = "text";
    inp.setAttribute("aria-label", `Condition ${n} value`);
    row.append(sel, inp);
    return { row, sel, inp };
  };
  const one = mkRow(1);
  const two = mkRow(2);

  const join = el("div", "filter-join");
  const radios = [];
  for (const [val, text] of [["and", "And"], ["or", "Or"]]) {
    const l = el("label");
    const r = document.createElement("input");
    r.type = "radio";
    r.name = "oc-filter-join";
    r.value = val;
    if (val === "and") r.checked = true;
    radios.push(r);
    l.append(r, document.createTextNode(" " + text));
    join.append(l);
  }

  const hint = el("div", "panel-hint", "Wildcards: * matches any characters, ? matches one.");
  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Apply");
  actions.append(cancel, ok);
  body.append(where, one.row, join, two.row, hint, actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    const [op1, v1] = encode(one.sel.value, one.inp.value);
    if (!one.inp.value) { status.textContent = "enter a value to compare against"; one.inp.focus(); return; }
    let op2 = "", v2 = "";
    if (two.sel.value && two.inp.value) [op2, v2] = encode(two.sel.value, two.inp.value);
    const and = radios.find((r) => r.checked).value === "and";
    close();
    canvas.focus();
    tryEdit(() => wasm.session_set_filter_custom(state.sheet, col, op1, v1, op2, v2, and));
    afterFilterChange();
  });
  one.inp.focus();
}
// --- Data validation (dropdown lists) -------------------------------------
// Open the value picker for the active cell's list validation.
function openValidationMenu() {
  if (!validationChevron) return;
  const rect = canvas.getBoundingClientRect();
  const x = rect.left + validationChevron.x;
  const y = rect.top + validationChevron.y + validationChevron.h;
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu dv-menu";
  menu.id = "sheet-ctx";
  validationChevron.values.forEach((val) => {
    const b = document.createElement("button");
    b.textContent = val;
    b.addEventListener("click", () => {
      closeSheetMenu();
      try { wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, val); }
      catch (e) { status.textContent = `error: ${e}`; }
      draw();
    });
    menu.appendChild(b);
  });
  positionMenu(menu, x, y);
}

// --- Tool side panel (data validation / conditional formatting / notes) ---
// One right-docked panel, tool-switched. It stays open while you keep selecting
// cells; the "Apply to range" readout tracks the live selection, and Apply acts
// on whatever is selected at click time.
let activePanel = null;        // 'dv' | 'cf' | 'note' | null
let panelRangeEls = [];        // range readouts to keep in sync on selection change
let panelNote = null;          // { ta, addrEl, cell } while the note panel is open

const A1range = (s) =>
  (s.r0 === s.r1 && s.c0 === s.c1) ? A1(s.r0, s.c0) : `${A1(s.r0, s.c0)}:${A1(s.r1, s.c1)}`;

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
function panelLabel(body, text) { body.appendChild(el("div", "panel-section-label", text)); }
function panelRangeReadout(body) {
  panelLabel(body, "Apply to range");
  const r = el("div", "panel-range", A1range(effectiveRange()));
  body.appendChild(r);
  panelRangeEls.push(r);
}
function panelActions(body, primaryText, onPrimary, ghostText, onGhost) {
  const row = el("div", "panel-actions");
  const ghost = el("button", "panel-btn-ghost", ghostText);
  ghost.addEventListener("click", onGhost);
  const primary = el("button", "panel-btn-primary", primaryText);
  primary.addEventListener("click", onPrimary);
  row.appendChild(ghost);
  row.appendChild(primary);
  body.appendChild(row);
  return primary;
}

function openPanel(tool) {
  const panel = document.getElementById("side-panel");
  activePanel = tool;
  panelRangeEls = [];
  panelNote = null;
  document.getElementById("side-panel-title").textContent =
    tool === "dv" ? "Data validation" : tool === "cf" ? "Conditional formatting" : "Note";
  const body = document.getElementById("side-panel-body");
  body.textContent = "";
  if (tool === "dv") buildDvPanel(body);
  else if (tool === "cf") buildCfPanel(body);
  else buildNotePanel(body);
  panel.hidden = false;
  resize(); // the grid narrows — refit the canvas to its new width
}

function closePanel() {
  const panel = document.getElementById("side-panel");
  if (panel.hidden) return;
  panel.hidden = true;
  activePanel = null;
  panelRangeEls = [];
  panelNote = null;
  resize();
  canvas.focus();
}

// Toggle a tool: clicking its button again (or a different tool) re-targets.
function togglePanel(tool) {
  if (activePanel === tool) closePanel();
  else openPanel(tool);
}

// Keep the panel in step with the selection (called from draw()).
function refreshPanel() {
  if (activePanel === "dv" || activePanel === "cf") {
    const t = A1range(effectiveRange());
    for (const r of panelRangeEls) r.textContent = t;
  } else if (activePanel === "note" && panelNote) {
    const addr = A1(state.sel.row, state.sel.col);
    if (addr !== panelNote.cell) {
      panelNote.cell = addr;
      panelNote.addrEl.textContent = addr;
      try { panelNote.ta.value = wasm.session_comment_at(state.sheet, state.sel.row, state.sel.col); } catch {}
    }
  }
}

function buildDvPanel(body) {
  panelRangeReadout(body);
  panelLabel(body, "Allow");
  // Every OOXML kind, not just the dropdown. The other kinds constrain what may
  // be typed; only `list` shows a picker.
  const kindSel = el("select", "panel-select");
  for (const [v, t] of [
    ["list", "List of values"], ["whole", "Whole number"], ["decimal", "Number"],
    ["date", "Date"], ["time", "Time"], ["textLength", "Text length"],
    ["custom", "Custom formula"], ["none", "Any value"],
  ]) { const o = el("option", null, t); o.value = v; kindSel.appendChild(o); }
  body.appendChild(kindSel);

  // List values.
  const inp = el("input", "panel-field");
  inp.placeholder = "Yes, No, Maybe";
  inp.spellcheck = false;
  const s0 = effectiveRange();
  try {
    const vj = wasm.session_validation_at(state.sheet, s0.r0, s0.c0);
    if (vj !== "null") inp.value = JSON.parse(vj).join(", ");
  } catch {}
  const listHint = el("div", "panel-hint", "Comma-separated. Cells in the range show a dropdown to pick from these values.");

  // Comparison operands, for the kinds that take them.
  const opSel = el("select", "panel-select");
  for (const [v, t] of [
    ["between", "between"], ["notBetween", "not between"], ["equal", "equal to"],
    ["notEqual", "not equal to"], ["greaterThan", "greater than"], ["lessThan", "less than"],
    ["greaterThanOrEqual", "at least"], ["lessThanOrEqual", "at most"],
  ]) { const o = el("option", null, t); o.value = v; opSel.appendChild(o); }
  const f1 = el("input", "panel-field"); f1.placeholder = "value"; f1.spellcheck = false;
  const f2 = el("input", "panel-field"); f2.placeholder = "and"; f2.spellcheck = false;
  const customHint = el("div", "panel-hint", "A formula that must be true, e.g. A1>0. Checked by the calc engine, not here.");
  body.append(inp, listHint, opSel, f1, f2, customHint);

  panelLabel(body, "If the value is rejected");
  const msg = el("input", "panel-field");
  msg.placeholder = "Optional message";
  msg.spellcheck = false;
  const blankWrap = el("label", "panel-check");
  const blank = document.createElement("input");
  blank.type = "checkbox";
  blank.checked = true;
  blankWrap.append(blank, document.createTextNode(" allow an empty cell"));
  body.append(msg, blankWrap);

  const sync = () => {
    const k = kindSel.value;
    const isList = k === "list";
    const isCustom = k === "custom";
    const isNone = k === "none";
    const cmp = !isList && !isCustom && !isNone;
    inp.style.display = isList ? "" : "none";
    listHint.style.display = isList ? "" : "none";
    opSel.style.display = cmp ? "" : "none";
    f1.style.display = cmp || isCustom ? "" : "none";
    f2.style.display = cmp && opSel.value.toLowerCase().includes("between") ? "" : "none";
    customHint.style.display = isCustom ? "" : "none";
    f1.placeholder = isCustom ? "A1>0" : k === "textLength" ? "length" : "value";
  };
  kindSel.addEventListener("change", sync);
  opSel.addEventListener("change", sync);
  sync();

  const apply = panelActions(
    body,
    "Apply",
    () => {
      const s = effectiveRange();
      try {
        if (kindSel.value === "list") {
          const vals = inp.value.split(",").map((x) => x.trim()).filter(Boolean);
          wasm.session_set_list_validation(state.sheet, s.r0, s.c0, s.r1, s.c1, vals);
        } else {
          wasm.session_set_validation(
            state.sheet, s.r0, s.c0, s.r1, s.c1,
            kindSel.value, opSel.value, f1.value, f2.value, blank.checked, msg.value);
        }
      } catch (e) { status.textContent = `error: ${e}`; }
      draw();
    },
    "Remove",
    () => {
      const s = effectiveRange();
      try { wasm.session_clear_validation(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
      draw();
    }
  );
  inp.addEventListener("keydown", (e) => { if (e.key === "Enter") apply.click(); });
  setTimeout(() => inp.focus(), 0);
}

function buildCfPanel(body) {
  panelRangeReadout(body);
  panelLabel(body, "Highlight cells where the value…");
  const op = el("select", "panel-select");
  [["gt", "is greater than"], ["lt", "is less than"], ["eq", "equals"], ["between", "is between"],
   ["contains", "text contains"],
   // Decided from the whole range rather than the cell alone.
   ["top", "is in the top N"], ["bottom", "is in the bottom N"],
   ["toppct", "is in the top N%"], ["bottompct", "is in the bottom N%"],
   ["above", "is above average"], ["below", "is below average"],
   ["duplicate", "is duplicated"], ["unique", "appears only once"],
   ["colorscale", "— colour scale (2 stops)"],
   ["colorscale3", "— colour scale (3 stops)"], ["databar", "— data bar"]]
    .forEach(([v, t]) => { const o = el("option", null, t); o.value = v; op.appendChild(o); });
  body.appendChild(op);
  const a = el("input", "panel-field"); a.placeholder = "value"; a.spellcheck = false;
  const b = el("input", "panel-field"); b.placeholder = "and"; b.spellcheck = false; b.style.display = "none";
  body.appendChild(a); body.appendChild(b);
  // The scale/bar kinds are range-relative: they take no operand, and their
  // colours come from the swatch row rather than a single fill.
  const rangeRelative = () => op.value.startsWith("colorscale") || op.value === "databar";
  // Kinds needing a rank, and kinds needing no operand at all.
  const ranked = () => ["top", "bottom", "toppct", "bottompct"].includes(op.value);
  const noOperand = () => rangeRelative() || ["above", "below", "duplicate", "unique"].includes(op.value);
  op.addEventListener("change", () => {
    b.style.display = op.value === "between" ? "" : "none";
    a.style.display = noOperand() ? "none" : "";
    a.placeholder = op.value === "contains" ? "text" : ranked() ? "how many" : "value";
    panelHint.textContent = rangeRelative()
      ? "Colour comes from the value's position between the range's smallest and largest."
      : ranked() || noOperand()
        ? "Compared against the whole range, so adding rows can change which cells match."
        : "";
  });
  const panelHint = el("div", "panel-hint");
  body.appendChild(panelHint);
  panelLabel(body, "Fill color");
  const strip = el("div", "panel-swatches");
  let fill = "ffd166";
  ["ffd166", "d1f0d6", "ffd6e0", "d6e4ff", "fed7aa", "e9d5ff", "fca5a5", "a7f3d0"].forEach((hx, i) => {
    const sw = el("button", "swatch" + (i === 0 ? " on" : ""));
    sw.style.background = "#" + hx;
    sw.title = "#" + hx;
    sw.addEventListener("click", () => { fill = hx; strip.querySelectorAll(".swatch").forEach((x) => x.classList.remove("on")); sw.classList.add("on"); });
    strip.appendChild(sw);
  });
  body.appendChild(strip);
  panelActions(
    body,
    "Apply",
    () => {
      const s = effectiveRange();
      let kind = op.value;
      const av = parseFloat(a.value) || 0, bv = parseFloat(b.value) || 0;
      // Scale and bar colours travel in the text slot: a scale needs two or
      // three, which the single fill slot cannot carry.
      let txt = kind === "contains" ? a.value : "";
      // A ranked rule's operand is a count, and it defaults to the top 10 —
      // Excel's own default, and the one the rule type is named after.
      const ranks = ["top", "bottom", "toppct", "bottompct"];
      const rank = ranks.includes(kind) ? Math.max(1, parseInt(a.value, 10) || 10) : av;
      if (kind === "colorscale") { txt = `${fill},ffffff`; }
      else if (kind === "colorscale3") { kind = "colorscale"; txt = `${fill},ffffff,63be7b`; }
      else if (kind === "databar") { txt = fill; }
      try { wasm.session_add_cf(state.sheet, s.r0, s.c0, s.r1, s.c1, kind, rank, bv, txt, fill); }
      catch (e) { status.textContent = `error: ${e}`; }
      draw();
    },
    "Clear",
    () => {
      const s = effectiveRange();
      try { wasm.session_clear_cf(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
      draw();
    }
  );
  setTimeout(() => a.focus(), 0);
}

function buildNotePanel(body) {
  panelLabel(body, "Note on cell");
  const addrEl = el("div", "panel-range", A1(state.sel.row, state.sel.col));
  body.appendChild(addrEl);
  const ta = el("textarea", "panel-field");
  ta.rows = 4; ta.spellcheck = false; ta.placeholder = "Type a note…";
  try { ta.value = wasm.session_comment_at(state.sheet, state.sel.row, state.sel.col); } catch {}
  body.appendChild(ta);
  body.appendChild(el("div", "panel-hint", "Notes attach to the active cell. Select another cell to edit its note."));
  panelNote = { ta, addrEl, cell: A1(state.sel.row, state.sel.col) };
  panelActions(
    body,
    "Save",
    () => {
      try { wasm.session_set_comment(state.sheet, state.sel.row, state.sel.col, ta.value); }
      catch (e) { status.textContent = `error: ${e}`; }
      draw();
    },
    "Delete",
    () => {
      try { wasm.session_set_comment(state.sheet, state.sel.row, state.sel.col, ""); } catch {}
      ta.value = "";
      draw();
    }
  );
  setTimeout(() => ta.focus(), 0);
}

// A yes/no question in the shared modal. Resolves true only on the confirm
// button; Escape, the ✕ and the backdrop all mean "no", because this is only
// used to guard destructive steps.
function confirmModal(title, message, confirmLabel = "OK") {
  return new Promise((resolve) => {
    const modal = document.getElementById("oc-modal");
    const body = document.getElementById("oc-modal-body");
    document.getElementById("oc-modal-title").textContent = title;
    body.textContent = "";
    const p = document.createElement("p");
    p.className = "oc-confirm-text";
    p.textContent = message;
    const row = document.createElement("div");
    row.className = "oc-confirm-actions";
    const cancel = document.createElement("button");
    cancel.className = "oc-btn";
    cancel.textContent = "Cancel";
    const ok = document.createElement("button");
    ok.className = "oc-btn primary";
    ok.textContent = confirmLabel;
    row.append(cancel, ok);
    body.append(p, row);
    modal.hidden = false;
    // The modal's own ✕ / backdrop wiring just hides it, which would leave this
    // promise pending and its key handler installed forever — so treat those
    // dismissals as "no" here too.
    const x = document.getElementById("oc-modal-x");
    const done = (answer) => {
      modal.hidden = true;
      body.textContent = "";
      document.removeEventListener("keydown", onKey, true);
      x.removeEventListener("click", onDismiss);
      modal.removeEventListener("click", onBackdrop);
      resolve(answer);
    };
    const onKey = (e) => {
      if (e.key === "Escape") { e.stopPropagation(); done(false); }
      else if (e.key === "Enter") { e.stopPropagation(); done(true); }
    };
    const onDismiss = () => done(false);
    const onBackdrop = (e) => { if (e.target === modal) done(false); };
    document.addEventListener("keydown", onKey, true);
    x.addEventListener("click", onDismiss);
    modal.addEventListener("click", onBackdrop);
    cancel.addEventListener("click", () => done(false));
    ok.addEventListener("click", () => done(true));
    ok.focus();
  });
}

// Excel's four merge verbs. "Across" merges each row of the selection
// separately — the one people reach for on a header band — and "& center"
// is a merge plus a centre, which is how it is nearly always used.
async function mergeVariant(kind) {
  const s = effectiveRange();
  if (kind === "none") {
    try { wasm.session_unmerge_cells(state.sheet, s.r0, s.c0, s.r1, s.c1); }
    catch (e) { status.textContent = `error: ${e}`; }
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
      } catch (e) { status.textContent = `error: ${e}`; }
    }
    status.textContent = `merged ${s.r1 - s.r0 + 1} rows across`;
    draw();
    return;
  }
  await toggleMerge();
  if (kind === "center") setAlign("center");
}

async function toggleMerge() {
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
    catch (e) { status.textContent = `error: ${e}`; }
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
  } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}
function setFontName(name) { formatSel((s) => wasm.session_set_font_name(state.sheet, s.r0, s.c0, s.r1, s.c1, name)); }
function setFontSize(pts) { formatSel((s) => wasm.session_set_font_size(state.sheet, s.r0, s.c0, s.r1, s.c1, pts)); }
// Grow/shrink font: step to the next/previous size on a standard ladder, based
// on the active cell's current size (default 11pt). Beyond the ladder, step ±2.
const SIZE_LADDER = [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48, 72];
function stepFontSize(dir) {
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
function setNumberFormat(code) { formatSel((s) => wasm.session_set_number_format(state.sheet, s.r0, s.c0, s.r1, s.c1, code)); }
function adjustDecimals(delta) { formatSel((s) => wasm.session_adjust_decimals(state.sheet, s.r0, s.c0, s.r1, s.c1, delta)); }
// Current border palette state (chosen line style + color, "" = automatic).
let borderStyle = "thin";
let borderColor = "";
function setBorder(kind) {
  // The composite bottoms are defined by their weight, so they carry their own
  // style rather than whatever the picker happens to be set to.
  const style = kind === "bottomdouble" ? "double"
    : kind === "bottomthick" ? "thick"
    : borderStyle;
  formatSel((s) => wasm.session_set_border(state.sheet, s.r0, s.c0, s.r1, s.c1, kind, style, borderColor));
}
function toggleBorder() { setBorder("all"); }

// Custom tooltips: convert native `title`s on the chrome to styled, faster
// tooltips (keeping an aria-label for a11y), shown on hover after a short delay.
let tipEl = null;
let tipTimer = 0;
function initTooltips() {
  tipEl = document.createElement("div");
  tipEl.className = "tooltip";
  tipEl.hidden = true;
  document.body.appendChild(tipEl);
  // Promote existing titles to data-tip so the native bubble doesn't also show.
  for (const node of document.querySelectorAll(".toolbar [title], .app-header [title], .formula-bar [title], .side-panel [title]")) {
    tipify(node);
  }
  document.addEventListener("mouseover", (e) => {
    const node = e.target.closest("[data-tip]");
    if (!node) return;
    clearTimeout(tipTimer);
    tipTimer = setTimeout(() => showTip(node), 380);
  });
  document.addEventListener("mouseout", (e) => {
    if (e.target.closest("[data-tip]")) hideTip();
  });
  document.addEventListener("mousedown", hideTip);
}
// Move an element's `title` to `data-tip` (+ aria-label), suppressing the native tip.
function tipify(node) {
  const t = node.getAttribute("title");
  if (!t) return;
  node.dataset.tip = t;
  if (!node.getAttribute("aria-label")) node.setAttribute("aria-label", t);
  node.removeAttribute("title");
}
function showTip(node) {
  if (!tipEl || !node.dataset.tip) return;
  tipEl.textContent = node.dataset.tip;
  tipEl.hidden = false;
  const r = node.getBoundingClientRect();
  const tw = tipEl.offsetWidth, th = tipEl.offsetHeight;
  let left = Math.max(6, Math.min(r.left + r.width / 2 - tw / 2, window.innerWidth - tw - 6));
  let top = r.bottom + 6;
  if (top + th > window.innerHeight - 6) top = r.top - th - 6;
  tipEl.style.left = left + "px";
  tipEl.style.top = top + "px";
  tipEl.classList.add("show");
}
function hideTip() {
  clearTimeout(tipTimer);
  if (tipEl) { tipEl.classList.remove("show"); tipEl.hidden = true; }
}

// A 20×20 icon sketching a cell with the placement's edges emphasized.
function bdIcon(kind) {
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
  for (const k in seg) faint += `<path d="${seg[k]}" stroke="var(--border)" stroke-width="1"/>`;
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
const BD_TITLES = {
  all: "All borders", inner: "Inner borders", outer: "Outer border",
  horizontal: "Inside horizontal", vertical: "Inside vertical", none: "Clear borders",
  top: "Top border", bottom: "Bottom border", left: "Left border", right: "Right border",
  topandbottom: "Top and bottom border", bottomdouble: "Bottom double border",
  bottomthick: "Thick bottom border",
  diagdown: "Diagonal down", diagup: "Diagonal up", diagboth: "Both diagonals",
  nodiag: "Clear diagonals",
};
// Build the border palette into #border-menu (once).
function buildBorderMenu() {
  const menu = document.getElementById("border-menu");
  menu.textContent = "";
  const grid = el("div", "bd-grid");
  for (const kind of ["all", "inner", "outer", "horizontal", "vertical", "none",
                      "top", "bottom", "left", "right",
                      "topandbottom", "bottomdouble", "bottomthick",
                      "diagdown", "diagup", "diagboth", "nodiag"]) {
    const b = el("button", "bd-cell");
    b.title = BD_TITLES[kind];
    b.setAttribute("aria-label", BD_TITLES[kind]);
    b.innerHTML = bdIcon(kind);
    b.addEventListener("click", (e) => { e.stopPropagation(); setBorder(kind); menu.hidden = true; canvas.focus(); });
    grid.appendChild(b);
  }
  menu.appendChild(grid);
  const sty = el("select", "tb-select bd-style");
  for (const [v, t] of [["thin", "Thin"], ["medium", "Medium"], ["thick", "Thick"], ["dashed", "Dashed"], ["dotted", "Dotted"], ["double", "Double"]]) {
    const o = el("option", null, t); o.value = v; sty.appendChild(o);
  }
  sty.value = borderStyle;
  sty.addEventListener("click", (e) => e.stopPropagation());
  sty.addEventListener("change", () => { borderStyle = sty.value; });
  menu.appendChild(sty);
  const sw = el("div", "bd-swatches");
  for (const c of ["", "000000", "2f6df6", "e5484d", "16a34a", "f59e0b", "8b5cf6", "64748b"]) {
    const b = el("button", "bd-color" + (c === borderColor ? " on" : ""));
    b.dataset.color = c;
    if (c) { b.style.background = "#" + c; b.title = "#" + c; }
    else { b.textContent = "A"; b.title = "Automatic color"; }
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      borderColor = c;
      sw.querySelectorAll(".bd-color").forEach((x) => x.classList.remove("on"));
      b.classList.add("on");
      const btn = document.getElementById("tb-border");
      btn.style.setProperty("--bd-color", c ? "#" + c : "currentColor");
    });
    sw.appendChild(b);
  }
  menu.appendChild(sw);
}
// Delete key / "Clear contents": clear values + formulas, keep formatting.
function clearSelection() {
  try { for (const s of allRanges()) wasm.session_clear_contents(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
  draw();
}
// "Clear formats": drop styling, keep values + formulas.
function clearFormats() {
  try { for (const s of allRanges()) wasm.session_clear_formats(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
  draw();
}
// "Clear all": also drop styles.
function clearAll() {
  try { for (const s of allRanges()) wasm.session_clear_range(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
  draw();
}
// --- Find & replace -------------------------------------------------------
const findBar = document.getElementById("find-bar");
const findInput = document.getElementById("find-input");
const replaceInput = document.getElementById("replace-input");
const findCount = document.getElementById("find-count");
const findCase = document.getElementById("find-case");
const findWhole = document.getElementById("find-whole");
const findValues = document.getElementById("find-values");
const findAllSheets = document.getElementById("find-all-sheets");
const findWildcards = document.getElementById("find-wildcards");
const findState = { matches: [], idx: -1 };

function openFind() { findBar.hidden = false; findInput.focus(); findInput.select(); runFind(); }
function closeFind() { findBar.hidden = true; canvas.focus(); }
function runFind() {
  const q = findInput.value;
  findState.matches = q
    ? JSON.parse(wasm.session_find_opts(
        state.sheet, q, findCase.checked,
        findWhole.checked, findValues.checked, findAllSheets.checked,
        findWildcards.checked))
    : [];
  findState.idx = findState.matches.length ? 0 : -1;
  if (findState.idx >= 0) gotoMatch();
  else { findCount.textContent = q ? "0" : ""; draw(); }
}
function gotoMatch() {
  const m = findState.matches[findState.idx];
  if (!m) return;
  // A whole-workbook search can land on another sheet; follow it there rather
  // than reporting a hit the user cannot see.
  if (m.s !== undefined && m.s !== state.sheet) switchSheet(m.s);
  select(m.r, m.c);
  findCount.textContent = `${findState.idx + 1}/${findState.matches.length}`;
}
function findStep(dir) {
  if (!findState.matches.length) return;
  findState.idx = (findState.idx + dir + findState.matches.length) % findState.matches.length;
  gotoMatch();
}
function replaceAll() {
  try {
    const n = wasm.session_replace_all(state.sheet, findInput.value, replaceInput.value, findCase.checked);
    status.textContent = `replaced ${n}`;
  } catch (e) { status.textContent = `error: ${e}`; }
  runFind();
}
// Replace only the current match, then re-search and jump to the next one.
function replaceOne() {
  const m = findState.matches[findState.idx];
  if (!m || !findInput.value) return;
  try {
    const did = wasm.session_replace_at(state.sheet, m.r, m.c, findInput.value, replaceInput.value, findCase.checked);
    status.textContent = did ? "replaced 1" : "no match here";
  } catch (e) { status.textContent = `error: ${e}`; }
  runFind();
}

// Undo/redo can add, remove, or reorder sheets, so rebuild the tab bar (which
// also re-clamps the active sheet if it vanished) before redrawing the grid.
function doUndo() { try { wasm.session_undo(); } catch {} renderTabs(); draw(); }
function doRedo() { try { wasm.session_redo(); } catch {} renderTabs(); draw(); }
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
// The TSV we last wrote to the OS clipboard. On paste we compare the OS
// clipboard to this: if it still matches, our richer internal snapshot is
// authoritative (formulas + styles); otherwise the user copied from elsewhere
// and we fall back to plain TSV.
let lastClipTsv = null;

// Marching-ants outline around the copy/cut source, animated by a dash offset.
let clipMarch = null;      // { sheet, r0, c0, r1, c1, cut } or null
let marchOffset = 0;
let marchRaf = 0;
let marchLast = 0;
function marchTick(t) {
  if (!clipMarch) { marchRaf = 0; return; }
  // Reduced motion: keep the dashed outline, drop the crawl. It marks the copy
  // source just as well standing still, and stopping the loop also stops a
  // repaint every 80 ms.
  if (REDUCED_MOTION.matches) { marchRaf = 0; return; }
  if (t - marchLast > 80) { marchOffset = (marchOffset + 1) % 8; marchLast = t; draw(); }
  marchRaf = requestAnimationFrame(marchTick);
}
// Honour `prefers-reduced-motion`: the marching ants are decoration, and the
// dashed outline says the same thing standing still.
const REDUCED_MOTION = window.matchMedia
  ? window.matchMedia("(prefers-reduced-motion: reduce)")
  : { matches: false };

function startMarch(s, cut) {
  clipMarch = { sheet: state.sheet, r0: s.r0, c0: s.c0, r1: s.r1, c1: s.c1, cut };
  if (!marchRaf) marchRaf = requestAnimationFrame(marchTick);
  draw();
}
function stopMarch() {
  if (!clipMarch) return;
  clipMarch = null;
  if (marchRaf) { cancelAnimationFrame(marchRaf); marchRaf = 0; }
  draw();
}

async function clipToOS(s, cut) {
  wasm.session_clip_copy(state.sheet, s.r0, s.c0, s.r1, s.c1, cut);
  startMarch(s, cut);
  const tsv = wasm.session_copy_tsv(state.sheet, s.r0, s.c0, s.r1, s.c1);
  lastClipTsv = tsv;
  // Write a formatted HTML table alongside the plain-text TSV so external apps
  // (Excel, Sheets, mail, docs) receive styling; fall back to text-only when
  // ClipboardItem isn't available or is blocked.
  try {
    const html = wasm.session_copy_html(state.sheet, s.r0, s.c0, s.r1, s.c1);
    if (navigator.clipboard.write && typeof ClipboardItem !== "undefined") {
      const item = new ClipboardItem({
        "text/html": new Blob([html], { type: "text/html" }),
        "text/plain": new Blob([tsv], { type: "text/plain" }),
      });
      await navigator.clipboard.write([item]);
      return true;
    }
  } catch { /* fall through to text-only */ }
  try { await navigator.clipboard.writeText(tsv); return true; }
  catch { return false; }
}
async function doCopy() {
  status.textContent = (await clipToOS(effectiveRange(), false)) ? "copied" : "copy blocked";
}
async function doCut() {
  status.textContent = (await clipToOS(effectiveRange(), true)) ? "cut" : "cut blocked";
}
// Paste-special: reproduce only part of the internal clipboard.
function doPasteMode(mode) {
  try {
    if (!wasm.session_clip_has()) { status.textContent = "clipboard is empty"; return; }
    wasm.session_clip_paste_mode(state.sheet, state.sel.row, state.sel.col, mode);
    if (!wasm.session_clip_has()) stopMarch(); // a cut was consumed
    draw();
    status.textContent = `pasted ${mode}`;
  } catch { status.textContent = "paste blocked"; }
}
async function doPaste() {
  try {
    let osText = "";
    try { osText = await navigator.clipboard.readText(); } catch {}
    // Internal rich paste when the OS clipboard is unchanged from our copy (or
    // unreadable but we hold a snapshot); else paste the external text.
    if (wasm.session_clip_has() && (osText === lastClipTsv || osText === "")) {
      wasm.session_clip_paste(state.sheet, state.sel.row, state.sel.col);
    } else {
      wasm.session_paste_tsv(state.sheet, state.sel.row, state.sel.col, osText);
    }
    if (!wasm.session_clip_has()) stopMarch(); // a cut was consumed
    draw();
    status.textContent = "pasted";
  } catch { status.textContent = "paste blocked"; }
}

// Place the in-cell editor over the cell being edited. Called on every redraw
// as well as on open: it is a DOM element over the canvas, so without this it
// stays parked where the cell *was* while the grid scrolls out from under it.
// A merged cell gets the whole block, not just its anchor's one-cell box.
function positionInline() {
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

// --- The edit session -------------------------------------------------------
// A cell can be edited from two surfaces: the in-cell overlay and the formula
// bar. `editSurface` is whichever <input> currently holds the edit, and every
// piece of formula intelligence below (autocomplete, click-to-insert a
// reference, the invalid-formula outline, commit/revert) is keyed off it rather
// than off the in-cell editor. Without this the formula bar is a dumb text box:
// the same typing that autocompletes in a cell does nothing up there.
let editSurface = null;
// The cell's text when the edit began, for Escape to restore.
let editOriginal = "";
// How the edit started: typing a fresh value ("Enter") or opening the existing
// one with F2 / double-click / the formula bar ("Edit"). Excel's status bar
// distinguishes these, and it is about the *gesture*, not about whether the
// cell happened to be empty.
let editMode = "Enter";
// The cell an in-progress edit belongs to, which reference picking can navigate
// away from.
let editHome = null;

function beginEdit(surface, initial) {
  editSurface = surface;
  state.editing = true;
  // Where this edit will be written. Reference picking may walk to another
  // sheet, so the target cannot simply be "wherever the selection is now".
  editHome = { sheet: state.sheet, row: state.sel.row, col: state.sel.col };
  editMode = initial !== undefined ? "Enter" : "Edit";
  refSpans = [];
  editOriginal = wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  if (surface === inline) {
    inline.style.display = "block";
    positionInline();
  }
  if (initial !== undefined) surface.value = initial;
  else surface.value = editOriginal;
  surface.focus();
  // Typing a character starts a fresh value; opening the editor selects what is
  // already there so the next keystroke replaces it.
  if (initial === undefined) surface.select();
  // Opening an existing formula highlights the cells it reads straight away,
  // rather than waiting for the first keystroke.
  updateRefSpans();
  updateCellMode();
}

function startInline(initial) {
  beginEdit(inline, initial);
}

// End the edit without committing. `refocus` false leaves focus alone — used
// when the caller is about to move it somewhere specific.
function endEdit(refocus = true) {
  const was = editSurface;
  editSurface = null;
  state.editing = false;
  // The range-finder outlines are painted into the canvas, so dropping them
  // needs a repaint — otherwise they linger over the grid after the edit ends.
  const hadSpans = refSpans.length > 0;
  refSpans = [];
  inline.style.display = "none";
  hideAutocomplete();
  hideSignatureTip();
  formulaRefDrag = null;
  pointMode = null;
  inline.classList.remove("invalid");
  fInput.classList.remove("invalid");
  if (refocus && was) canvas.focus();
  updateCellMode();
  if (hadSpans) draw();
}
// Kept as the name the rest of the editor already calls.
function endInline() {
  endEdit();
}
// --- Range finder -----------------------------------------------------------
// While a formula is being edited, each reference in it gets a colored outline
// on the grid, so "which cells does this formula actually read?" is answerable
// by looking rather than by parsing the text in your head. The spans come from
// the engine's scanner, so a function name is never mistaken for a reference.
let refSpans = []; // [{s,e,r0,c0,r1,c1,sh}] for the formula being edited
// Excel/Sheets use a small rotating palette, one color per distinct reference.
const REF_COLORS = ["#1a73e8", "#e37400", "#0f9d58", "#a142f4", "#d93025", "#12b5cb"];

function updateRefSpans() {
  const text = editSurface ? editSurface.value : "";
  const next = text.startsWith("=") && wasm
    ? (() => { try { return JSON.parse(wasm.formula_ref_spans(text)); } catch { return []; } })()
    : [];
  // Only repaint when the set actually changed — this runs on every keystroke.
  const same = next.length === refSpans.length &&
    next.every((r, i) => {
      const p = refSpans[i];
      return p && r.r0 === p.r0 && r.c0 === p.c0 && r.r1 === p.r1 && r.c1 === p.c1 && r.sh === p.sh;
    });
  refSpans = next;
  if (!same) draw();
}

// Arrow-key point mode: with the caret somewhere a reference may go, the arrow
// keys build one by moving a pointer over the grid instead of moving the text
// caret — Excel's "point mode". Shift extends it into a range. The state mirrors
// `formulaRefDrag`, so insertRef keeps replacing the same span of text.
let pointMode = null; // {anchor:{row,col}, cur:{row,col}, start, end}

function pointStep(dr, dc, extend) {
  if (!editSurface) return false;
  if (!pointMode) {
    if (!refAcceptable()) return false;
    // Excel starts from the cell being edited and steps off it.
    const cur = {
      row: Math.max(0, state.sel.row + dr),
      col: Math.max(0, state.sel.col + dc),
    };
    const at = editSurface.selectionStart;
    pointMode = { anchor: cur, cur, start: at, end: at };
  } else {
    const cur = {
      row: Math.max(0, pointMode.cur.row + dr),
      col: Math.max(0, pointMode.cur.col + dc),
    };
    pointMode.cur = cur;
    if (!extend) pointMode.anchor = cur; // a plain arrow moves the whole point
  }
  const a = pointMode.anchor, c = pointMode.cur;
  const r0 = Math.min(a.row, c.row), r1 = Math.max(a.row, c.row);
  const c0 = Math.min(a.col, c.col), c1 = Math.max(a.col, c.col);
  insertRef(r0 === r1 && c0 === c1 ? A1(r0, c0) : `${A1(r0, c0)}:${A1(r1, c1)}`);
  // Keep the cell being pointed at on screen, without disturbing the selection.
  ensureVisible(c.row, c.col);
  draw();
  return true;
}

// F4 cycles the anchoring of the reference under the caret, in Excel's order:
// A1 → $A$1 → A$1 → $A1 → A1. A range cycles both endpoints together, and a
// sheet qualifier is left alone. Returns whether anything was rewritten.
const A1_PART = /^(\$?)([A-Za-z]+)(\$?)([0-9]+)$/;
function cycleAnchors() {
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

// Excel shows the in-progress text on both surfaces at once. Mirror the one
// being typed in onto the other — never the reverse, or the two carets fight.
function mirrorEdit() {
  if (!editSurface) return;
  if (editSurface === inline) fInput.value = inline.value;
  else inline.value = fInput.value;
}

// Abandon the edit and put the cell's own text back on both surfaces.
function cancelEdit() {
  if (editHome && editHome.sheet !== state.sheet) {
    switchSheet(editHome.sheet, true);
    state.sel = { row: editHome.row, col: editHome.col };
    state.anchor = { ...state.sel };
  }
  if (editSurface) editSurface.value = editOriginal;
  endEdit();
  if (wasm) refreshFormulaBar();
}

// --- Formula editing UX: autocomplete, reference insertion, validation -----
const A1 = (row, col) => colName(col) + (row + 1);

// --- Name box (cell-ref input): show address / drag size, jump on Enter -----
// Reflect the selection into the name box unless the user is typing in it. While
// drag-selecting a block, show Excel's "3R x 2C" size readout.
// --- Assistive announcements + cell mode -----------------------------------
// The grid is a canvas: nothing in it is in the accessibility tree. This
// announces the active cell and what it holds as the selection moves, so the
// grid is at least navigable with a screen reader — an announcer, not a full
// grid tree.
const liveEl = document.getElementById("grid-live");
const modeEl = document.getElementById("cell-mode");
let lastAnnounced = "";
function announceCell() {
  if (!liveEl || !wasm) return;
  const ref = A1(state.sel.row, state.sel.col);
  const r = selRect();
  const size = r.r0 === r.r1 && r.c0 === r.c1
    ? ""
    : `, ${r.r1 - r.r0 + 1} by ${r.c1 - r.c0 + 1} selected`;
  let text = "";
  try {
    const it = JSON.parse(wasm.session_cells(state.sheet, state.sel.row, state.sel.col, state.sel.row, state.sel.col))[0];
    text = it && it.t ? it.t : "empty";
  } catch { text = ""; }
  const msg = `${ref}${size}. ${text}`;
  // Re-announcing the same string is silence in most screen readers, and noise
  // in the rest; only speak on an actual change.
  if (msg === lastAnnounced) return;
  lastAnnounced = msg;
  liveEl.textContent = msg;
}

// Excel's status-bar mode word. Ready → Enter (typing a fresh value) → Edit
// (F2 into an existing one) → Point (picking a reference mid-formula).
function updateCellMode() {
  if (!modeEl) return;
  let mode = "Ready";
  if (editSurface) mode = pointMode || formulaRefDrag ? "Point" : editMode;
  if (modeEl.textContent !== mode) modeEl.textContent = mode;
}

function updateNameBox() {
  if (document.activeElement === cellRef) return;
  const r = selRect();
  const rows = r.r1 - r.r0 + 1, cols = r.c1 - r.c0 + 1;
  if ((state.dragging || formulaRefDrag) && (rows > 1 || cols > 1)) {
    cellRef.value = `${rows}R x ${cols}C`;
  } else {
    cellRef.value = A1(state.sel.row, state.sel.col);
  }
}
// "AB" -> zero-based column index, or null.
function colFromLetters(s) {
  let n = 0;
  for (const ch of s.toUpperCase()) {
    if (ch < "A" || ch > "Z") return null;
    n = n * 26 + (ch.charCodeAt(0) - 64);
  }
  return n > 0 ? n - 1 : null;
}
// "B12" -> {row,col}, or null.
function parseA1Cell(s) {
  const m = /^([A-Za-z]+)([0-9]+)$/.exec(s.trim());
  if (!m) return null;
  const col = colFromLetters(m[1]);
  const row = parseInt(m[2], 10) - 1;
  return col === null || row < 0 || !Number.isFinite(row) ? null : { row, col };
}
// Jump to a typed cell (B12) or range (A1:C5). Unknown names report to status.
function gotoName(v) {
  const s = (v || "").trim();
  if (!s) { updateNameBox(); return; }
  const parts = s.split(":");
  if (parts.length === 2) {
    const a = parseA1Cell(parts[0]), b = parseA1Cell(parts[1]);
    if (a && b) {
      state.anchor = { row: a.row, col: a.col };
      state.sel = { row: b.row, col: b.col };
      state.selKind = "cells";
      state.ranges = [];
      ensureVisible();
      draw();
      return;
    }
  } else {
    const c = parseA1Cell(s);
    if (c) { select(c.row, c.col); return; }
  }
  // An existing defined name → jump to its target range.
  try {
    const t = wasm.session_name_target(s);
    if (t !== "null") {
      const r = JSON.parse(t);
      state.anchor = { row: r.r0, col: r.c0 };
      state.sel = { row: r.r1, col: r.c1 };
      state.selKind = "cells";
      state.ranges = [];
      ensureVisible();
      draw();
      return;
    }
  } catch {}
  // A valid new name → define it for the current selection (Excel's name box).
  if (/^[A-Za-z_][A-Za-z0-9_.]*$/.test(s)) {
    const r = effectiveRange();
    const names = JSON.parse(wasm.session_sheet_names());
    const sn = names[state.sheet] || "Sheet1";
    const q = /[^A-Za-z0-9_]/.test(sn) ? `'${sn.replace(/'/g, "''")}'` : sn;
    const refers = `${q}!${A1(r.r0, r.c0)}:${A1(r.r1, r.c1)}`;
    try { wasm.session_define_name(s, refers); status.textContent = `defined name “${s}”`; }
    catch (e) { status.textContent = `error: ${e}`; }
    updateNameBox();
    return;
  }
  status.textContent = `Can't go to “${s}” — type a cell (B12), range (A1:C5), or a name`;
  updateNameBox();
}
// Ctrl+F3 Name Manager: list defined names to navigate to or delete.
function openNameManager(x, y) {
  closeSheetMenu();
  let names = [];
  try { names = JSON.parse(wasm.session_names()); } catch {}
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu nm-menu";
  menu.id = "sheet-ctx";
  const head = document.createElement("div");
  head.className = "menu-label";
  head.textContent = names.length ? "Named ranges" : "No named ranges yet";
  menu.appendChild(head);
  names.forEach((n) => {
    const row = document.createElement("div");
    row.className = "nm-row";
    const go = document.createElement("button");
    go.className = "nm-go";
    go.innerHTML = `<b>${n.name}</b><span>${n.refersTo}</span>`;
    go.addEventListener("click", () => { closeSheetMenu(); gotoName(n.name); });
    const del = document.createElement("button");
    del.className = "nm-del";
    del.textContent = "×";
    del.title = "Delete";
    del.addEventListener("click", (e) => { e.stopPropagation(); try { wasm.session_delete_name(n.name); } catch {} row.remove(); draw(); });
    row.appendChild(go); row.appendChild(del);
    menu.appendChild(row);
  });
  positionMenu(menu, x, y);
}
// Whether the caret (end of `before`) sits inside a "..." string literal, so we
// must not inject a cell reference or a function name there. Treats "" as an
// escaped quote within a string.
function inStringLiteral(before) {
  let inStr = false;
  for (let i = 0; i < before.length; i++) {
    if (before[i] === '"') {
      if (inStr && before[i + 1] === '"') { i++; continue; }
      inStr = !inStr;
    }
  }
  return inStr;
}
let fnCatalog = null;            // lazily-loaded function catalog
let acState = null;             // active autocomplete: {matches, idx, start}
let formulaRefDrag = null;      // click/drag ref insertion: {anchor, start, end}
const acEl = document.getElementById("ac-menu");

// The function-name token being typed just before the caret, if the caret sits
// somewhere a function name is valid (after =, an operator, "(", or ",").
function currentFnToken() {
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

function showAutocomplete() {
  const tok = currentFnToken();
  if (!tok) { hideAutocomplete(); return; }
  if (!fnCatalog) { try { fnCatalog = JSON.parse(wasm.function_catalog()); } catch { fnCatalog = []; } }
  const up = tok.text.toUpperCase();
  const matches = fnCatalog.filter((f) => f.n.startsWith(up)).slice(0, 8);
  if (!matches.length) { hideAutocomplete(); return; }
  acState = { matches, idx: 0, start: tok.start };
  renderAutocomplete();
}

function renderAutocomplete() {
  if (!acState) return;
  acEl.textContent = "";
  acState.matches.forEach((f, i) => {
    const row = document.createElement("div");
    row.className = "ac-item" + (i === acState.idx ? " active" : "");
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

function hideAutocomplete() { acState = null; if (acEl) acEl.hidden = true; }

// --- Argument hint ----------------------------------------------------------
// Once the caret is inside a function's parentheses the name list is no longer
// what you need — the question becomes "which argument am I typing?". This
// shows the signature with that argument emphasised, the way Excel and Sheets
// do, and follows the caret through nested calls.
const sigEl = document.getElementById("sig-tip");

// The innermost call the caret sits inside: its function name and the index of
// the argument being typed. Commas inside nested calls or string literals do
// not count, which is the whole difficulty.
function callAtCaret() {
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

function updateSignatureTip() {
  // The name list takes precedence: both anchored to the caret would collide,
  // and while it is open the user is still choosing a function.
  if (!sigEl) return;
  const call = acState ? null : callAtCaret();
  if (!call) { sigEl.hidden = true; return; }
  if (!fnCatalog) { try { fnCatalog = JSON.parse(wasm.function_catalog()); } catch { fnCatalog = []; } }
  const fn = fnCatalog.find((f) => f.n === call.name);
  if (!fn) { sigEl.hidden = true; return; }
  const inner = /\(([^]*)\)\s*$/.exec(fn.sig);
  const args = inner ? inner[1].split(",").map((a) => a.trim()) : [];
  sigEl.textContent = "";
  const name = document.createElement("span");
  name.className = "sig-name";
  name.textContent = fn.n;
  sigEl.append(name, document.createTextNode("("));
  args.forEach((a, i) => {
    if (i) sigEl.append(document.createTextNode(", "));
    const span = document.createElement("span");
    // A trailing "…" argument keeps matching once the caret runs past the
    // named ones, which is what makes SUM(a, b, c, …) highlight sensibly.
    const variadic = i === args.length - 1 && /^[.…]/.test(a);
    span.className = "sig-arg" + (i === call.arg || (variadic && call.arg > i) ? " active" : "");
    span.textContent = a;
    sigEl.appendChild(span);
  });
  sigEl.append(document.createTextNode(")"));
  sigEl.hidden = false;
  const r = (editSurface ?? inline).getBoundingClientRect();
  const w = sigEl.offsetWidth;
  sigEl.style.left = Math.max(4, Math.min(r.left, window.innerWidth - 4 - w)) + "px";
  sigEl.style.top = r.bottom + 2 + "px";
}
function hideSignatureTip() { if (sigEl) sigEl.hidden = true; }

function acceptAutocomplete() {
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

// Whether the caret sits where a cell reference may be inserted by clicking.
function refAcceptable() {
  if (!editSurface || !editSurface.value.startsWith("=")) return false;
  const raw = editSurface.value.slice(0, editSurface.selectionStart);
  if (inStringLiteral(raw)) return false; // caret inside a "text" literal
  const before = raw.trimEnd();
  if (before === "=") return true;
  return "=+-*/^(,:&<>% ".includes(before[before.length - 1]);
}

// Insert a reference at the caret while editing. While a gesture that keeps
// adjusting the same reference is in progress — a mouse drag, or arrow-key
// point mode — the previously inserted text is replaced rather than appended.
function insertRef(text) {
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

const tabsEl = document.getElementById("sheet-tabs");

// Reset the viewport + selection to the top-left (e.g. on a sheet switch).
function resetView() {
  state.scrollX = state.scrollY = 0;
  state.sel = { row: 0, col: 0 };
  state.anchor = { row: 0, col: 0 };
  endInline();
  draw();
}

// Per-sheet view memory: switching sheets preserves each sheet's selection and
// scroll position (Excel/Sheets behavior) instead of slamming back to A1. Keyed
// by sheet NAME so it survives add/delete/reorder/undo without index-shift bugs
// (a rename just drops that one sheet's remembered view — acceptable).
const sheetViews = new Map(); // sheet name → { scrollX, scrollY, sel, anchor, selKind }
function sheetNameAt(i) {
  try { return JSON.parse(wasm.session_sheet_names())[i]; } catch { return null; }
}
function saveSheetView() {
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
// `keepEdit` leaves an in-progress edit open — used when a formula is picking a
// reference on another sheet, where switching sheets is part of *authoring* the
// formula rather than abandoning it.
function switchSheet(i, keepEdit = false) {
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
}

// (Re)build the bottom sheet-tab bar from the engine's sheet list.
function renderTabs() {
  const names = JSON.parse(wasm.session_sheet_names());
  let vis = [];
  try { vis = JSON.parse(wasm.session_sheet_visibility()); } catch {}
  let prot = [];
  try { prot = JSON.parse(wasm.session_sheet_protected()); } catch {}
  if (state.sheet >= names.length) state.sheet = names.length - 1;
  // Never sit on a hidden sheet — the tab is gone, so there would be no way
  // back to a visible one.
  if (vis[state.sheet] && vis[state.sheet] !== "visible") {
    const firstVisible = names.findIndex((_, i) => (vis[i] || "visible") === "visible");
    if (firstVisible >= 0) state.sheet = firstVisible;
  }
  tabsEl.textContent = "";
  names.forEach((name, i) => {
    // Hidden sheets keep their index (everything addresses sheets by it) but
    // get no tab; they are reachable through the tab context menu.
    if ((vis[i] || "visible") !== "visible") return;
    const b = document.createElement("button");
    b.className = "sheet-tab" + (i === state.sheet ? " active" : "");
    b.textContent = name;
    let tc = "";
    try { tc = wasm.session_tab_color(i); } catch (_) {}
    if (tc) {
      b.style.setProperty("--tab-color", "#" + tc);
      b.classList.add("colored");
    }
    if (prot[i]) {
      // A protected sheet reads the same as any other until you try to edit it,
      // so say so on the tab itself.
      b.classList.add("protected");
      b.title = name + " (protected)";
    }
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", i === state.sheet ? "true" : "false");
    b.addEventListener("mousedown", (e) => {
      // Mid-formula, clicking a tab is part of writing the formula: keep the
      // edit alive and do not let the click blur the editor.
      if (refAcceptable()) { e.preventDefault(); switchSheet(i, true); }
    });
    b.addEventListener("click", () => { if (!editSurface) switchSheet(i); });
    b.addEventListener("dblclick", () => renameSheet(i, b));
    b.addEventListener("contextmenu", (e) => { e.preventDefault(); sheetMenu(i, e.clientX, e.clientY); });
    // Drag to reorder.
    b.draggable = true;
    b.addEventListener("dragstart", (e) => { dragTab = i; e.dataTransfer.effectAllowed = "move"; b.classList.add("dragging"); });
    b.addEventListener("dragend", () => { dragTab = -1; b.classList.remove("dragging"); });
    b.addEventListener("dragover", (e) => { e.preventDefault(); e.dataTransfer.dropEffect = "move"; });
    b.addEventListener("drop", (e) => { e.preventDefault(); moveTab(dragTab, i); });
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

  // All-sheets menu. The strip scrolls once there are more tabs than fit, but
  // scrolling only helps if you already know where you are going — this lists
  // every sheet, hidden ones included, and jumps straight to it.
  const all = document.createElement("button");
  all.className = "sheet-add sheet-all";
  all.title = "All sheets";
  all.setAttribute("aria-label", "All sheets");
  all.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="icon-sm"><line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/></svg>';
  all.addEventListener("click", (e) => {
    e.stopPropagation();
    closeSheetMenu();
    const menu = document.createElement("div");
    menu.className = "popmenu ctx-menu";
    menu.id = "sheet-ctx";
    names.forEach((n, idx) => {
      const hiddenHere = (vis[idx] || "visible") !== "visible";
      const b = document.createElement("button");
      b.className = "menu-item" + (idx === state.sheet ? " on" : "");
      b.textContent = n + (hiddenHere ? "  (hidden)" : "") + (prot[idx] ? "  🔒" : "");
      b.addEventListener("click", () => {
        closeSheetMenu();
        // Jumping to a hidden sheet reveals it: the alternative is switching to
        // something with no tab and no way back.
        if (hiddenHere) {
          try { wasm.session_set_sheet_visibility(idx, "visible"); } catch {}
        }
        switchSheet(idx);
        renderTabs();
        draw();
      });
      menu.appendChild(b);
    });
    const r = all.getBoundingClientRect();
    positionMenu(menu, r.left, r.top - 4);
  });
  tabsEl.appendChild(all);

  // Keep the active tab in view once the strip overflows.
  const activeTab = tabsEl.querySelector(".sheet-tab.active");
  if (activeTab) activeTab.scrollIntoView({ block: "nearest", inline: "nearest" });
}

// Reorder sheet tabs, keeping the active sheet tracked through the shift.
function moveTab(from, to) {
  if (from < 0 || from === to) return;
  try { wasm.session_move_sheet(from, to); } catch (e) { status.textContent = `error: ${e}`; return; }

  if (state.sheet === from) state.sheet = to;
  else {
    let a = state.sheet > from ? state.sheet - 1 : state.sheet;
    if (a >= to) a += 1;
    state.sheet = a;
  }
  renderTabs();
  draw();
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
  // Tab-color swatch strip.
  const sep = document.createElement("div");
  sep.className = "menu-sep";
  menu.appendChild(sep);
  const lbl = document.createElement("div");
  lbl.className = "menu-label";
  lbl.textContent = "Tab color";
  menu.appendChild(lbl);
  const strip = document.createElement("div");
  strip.className = "swatch-row";
  const setTabColor = (hex) => {
    closeSheetMenu();
    try { wasm.session_set_tab_color(i, hex); renderTabs(); }
    catch (e) { status.textContent = `error: ${e}`; }
  };
  ["E53935", "FB8C00", "FDD835", "43A047", "1E88E5", "5E35B1", "8E24AA", "546E7A"].forEach((hex) => {
    const sw = document.createElement("button");
    sw.className = "swatch";
    sw.style.background = "#" + hex;
    sw.title = "#" + hex;
    sw.addEventListener("click", (e) => { e.stopPropagation(); setTabColor(hex); });
    strip.appendChild(sw);
  });
  const none = document.createElement("button");
  none.className = "swatch swatch-none";
  none.title = "No color";
  none.addEventListener("click", (e) => { e.stopPropagation(); setTabColor(""); });
  strip.appendChild(none);
  menu.appendChild(strip);
  menu.appendChild(document.createElement("div")).className = "menu-sep";
  let prot = [];
  try { prot = JSON.parse(wasm.session_sheet_protected()); } catch {}
  item(prot[i] ? "Unprotect sheet" : "Protect sheet", false, () => {
    try { wasm.session_set_sheet_protected(i, !prot[i]); renderTabs(); draw(); }
    catch (e) { status.textContent = `error: ${e}`; }
    status.textContent = prot[i] ? "sheet unprotected" : "sheet protected";
  });
  item("Hide sheet", false, () => {
    try { wasm.session_set_sheet_visibility(i, "hidden"); renderTabs(); draw(); }
    catch (e) { status.textContent = `${e}`.replace(/^Error:\s*/, ""); }
  });
  // Unhide lists the hidden sheets by name, since they have no tab to click.
  let vis = [];
  try { vis = JSON.parse(wasm.session_sheet_visibility()); } catch {}
  const names = JSON.parse(wasm.session_sheet_names());
  // `veryHidden` is deliberately absent: Excel does not offer it here either,
  // and silently promoting it to merely hidden would undo the author's choice.
  const hidden = names
    .map((n, idx) => ({ n, idx }))
    .filter(({ idx }) => vis[idx] === "hidden");
  if (hidden.length) {
    for (const { n, idx } of hidden) {
      item(`Unhide "${n}"`, false, () => {
        try { wasm.session_set_sheet_visibility(idx, "visible"); renderTabs(); draw(); }
        catch (e) { status.textContent = `${e}`; }
      });
    }
    menu.appendChild(document.createElement("div")).className = "menu-sep";
  }
  item("Delete", true, () => {
    try {
      wasm.session_delete_sheet(i);
      if (i <= state.sheet) state.sheet = Math.max(0, state.sheet - 1);
      renderTabs();
      resetView();
    } catch (e) { status.textContent = `error: ${e}`; }
  });
  positionMenu(menu, x, y);
}

// Append a context menu at (x,y), flipping up/left if it would overflow.
function positionMenu(menu, x, y) {
  menu.style.left = "0px";
  menu.style.top = "0px";
  menu.style.visibility = "hidden";
  document.body.appendChild(menu);
  const h = menu.offsetHeight, w = menu.offsetWidth;
  menu.style.top = (y + h > window.innerHeight ? Math.max(4, y - h) : y) + "px";
  menu.style.left = (x + w > window.innerWidth ? Math.max(4, x - w) : x) + "px";
  menu.style.visibility = "visible";
  setTimeout(() => document.addEventListener("click", closeSheetMenu, { once: true }), 0);
}

function tryEdit(fn) {
  try { fn(); } catch (e) { status.textContent = `error: ${e}`; }
  // Any edit can add, remove or re-wrap a grown row, so the growth map — and
  // every offset derived from it — has to be rebuilt.
  invalidateGrowth();
  draw();
}

// Ctrl +/- structural edits, axis chosen by the selection kind: whole-column
// selection acts on columns, whole-row on rows, otherwise rows (Excel's default
// for a cell selection). `count` spans the selection.
function insertLines() {
  const r = effectiveRange();
  const rn = r.r1 - r.r0 + 1, cn = r.c1 - r.c0 + 1;
  if (state.selKind === "cols") tryEdit(() => wasm.session_insert_columns(state.sheet, r.c0, cn));
  else tryEdit(() => wasm.session_insert_rows(state.sheet, r.r0, rn));
}
function deleteLines() {
  const r = effectiveRange();
  const rn = r.r1 - r.r0 + 1, cn = r.c1 - r.c0 + 1;
  if (state.selKind === "cols") tryEdit(() => wasm.session_delete_columns(state.sheet, r.c0, cn));
  else tryEdit(() => wasm.session_delete_rows(state.sheet, r.r0, rn));
}

// Right-click menu on a cell: clipboard + structural row/column edits.
// Trimmed right-click menu: fast verbs only. Multi-option groups (Paste
// special, Insert, Delete, Hide, Clear, Sort) fold into submenus so the menu
// stays short; the heavier editors (validation / conditional format / notes)
// live in the side panel, reached from the toolbar.
function cellMenu(x, y) {
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu";
  menu.id = "sheet-ctx";
  const hideSubs = () => menu.querySelectorAll(".ctx-submenu").forEach((s) => (s.hidden = true));
  const sep = () => menu.appendChild(el("div", "menu-sep"));
  const item = (label, danger, fn) => {
    const b = el("button", danger ? "danger" : null, label);
    b.addEventListener("mouseenter", hideSubs);
    b.addEventListener("click", () => { closeSheetMenu(); fn(); });
    menu.appendChild(b);
  };
  // A submenu row (label + ›); its child popmenu is nested so it is removed
  // with the parent, and fixed-positioned to the parent row's right edge.
  const submenu = (label, entries) => {
    const b = el("button", "has-sub", label);
    b.setAttribute("aria-haspopup", "true");
    const sub = el("div", "popmenu ctx-submenu");
    sub.hidden = true;
    for (const [lbl, danger, fn] of entries) {
      const c = el("button", danger ? "danger" : null, lbl);
      c.addEventListener("click", (e) => { e.stopPropagation(); closeSheetMenu(); fn(); });
      sub.appendChild(c);
    }
    const openSub = () => {
      hideSubs();
      sub.hidden = false;
      const r = b.getBoundingClientRect();
      const sw = sub.offsetWidth, sh = sub.offsetHeight;
      let left = r.right - 2;
      if (left + sw > window.innerWidth - 4) left = Math.max(4, r.left - sw + 2);
      let top = r.top - 4;
      if (top + sh > window.innerHeight - 4) top = Math.max(4, window.innerHeight - 4 - sh);
      sub.style.left = left + "px";
      sub.style.top = top + "px";
    };
    b.addEventListener("mouseenter", openSub);
    b.addEventListener("click", (e) => { e.stopPropagation(); sub.hidden ? openSub() : (sub.hidden = true); });
    menu.appendChild(b);
    menu.appendChild(sub);
  };
  const span = () => { const r = effectiveRange(); return { r, rows: r.r1 - r.r0 + 1, cols: r.c1 - r.c0 + 1 }; };

  item("Cut", false, () => doCut());
  item("Copy", false, () => doCopy());
  item("Paste", false, () => doPaste());
  submenu("Paste special", [
    ["Values only", false, () => doPasteMode("values")],
    ["Formulas only", false, () => doPasteMode("formulas")],
    ["Formats only", false, () => doPasteMode("formats")],
    ["Transpose", false, () => doPasteMode("transpose")],
  ]);
  sep();
  submenu("Insert", [
    ["Row above", false, () => { const { r, rows } = span(); tryEdit(() => wasm.session_insert_rows(state.sheet, r.r0, rows)); }],
    ["Row below", false, () => { const { r, rows } = span(); tryEdit(() => wasm.session_insert_rows(state.sheet, r.r1 + 1, rows)); }],
    ["Column left", false, () => { const { r, cols } = span(); tryEdit(() => wasm.session_insert_columns(state.sheet, r.c0, cols)); }],
    ["Column right", false, () => { const { r, cols } = span(); tryEdit(() => wasm.session_insert_columns(state.sheet, r.c1 + 1, cols)); }],
  ]);
  submenu("Delete", [
    ["Row", true, () => { const { r, rows } = span(); tryEdit(() => wasm.session_delete_rows(state.sheet, r.r0, rows)); }],
    ["Column", true, () => { const { r, cols } = span(); tryEdit(() => wasm.session_delete_columns(state.sheet, r.c0, cols)); }],
  ]);
  submenu("Hide", [
    ["Row", false, () => { const { r } = span(); tryEdit(() => wasm.session_hide_rows(state.sheet, r.r0, r.r1)); }],
    ["Column", false, () => { const { r } = span(); tryEdit(() => wasm.session_hide_cols(state.sheet, r.c0, r.c1)); }],
    ["Unhide rows/cols", false, () => { const { r } = span(); tryEdit(() => { wasm.session_unhide_rows(state.sheet, r.r0, r.r1); wasm.session_unhide_cols(state.sheet, r.c0, r.c1); }); }],
  ]);
  sep();
  submenu("Clear", [
    ["Contents", false, () => clearSelection()],
    ["Formats", false, () => clearFormats()],
    ["All (incl. formats)", true, () => clearAll()],
  ]);
  submenu("Sort", [
    [`${colName(state.sel.col)} A → Z`, false, () => sortRange(false)],
    [`${colName(state.sel.col)} Z → A`, false, () => sortRange(true)],
    ["Custom sort…", false, () => sortDialog()],
  ]);
  positionMenu(menu, x, y);
}

// Ask for a row height / column width in pixels, seeded with the current one.
// A plain prompt rather than a styled modal: it is a single number, and the
// alternative was no way to set an exact size at all.
function sizeDialog(axis, index) {
  const isCol = axis === "col";
  let current = 0;
  try {
    current = isCol
      ? JSON.parse(wasm.session_col_px(state.sheet, index, 1))[0]
      : JSON.parse(wasm.session_row_px(state.sheet, index, 1))[0];
  } catch {}
  const label = isCol ? `Column ${colName(index)} width (px)` : `Row ${index + 1} height (px)`;
  const answer = window.prompt(label, String(current || (isCol ? COL_W : ROW_H)));
  if (answer === null) return;
  const px = Math.round(parseFloat(answer));
  if (!Number.isFinite(px) || px < 0) { status.textContent = "not a size"; return; }
  const r = selRect();
  tryEdit(() => {
    if (isCol) for (let c = r.c0; c <= r.c1; c += 1) wasm.session_set_col_width(state.sheet, c, px);
    else for (let row = r.r0; row <= r.r1; row += 1) wasm.session_set_row_height(state.sheet, row, px);
  });
}

// The context menu for a row or column header. Same chrome as the cell menu,
// but every verb names — and acts on — the band that was right-clicked.
function headerMenu(axis, x, y) {
  closeSheetMenu();
  const isCol = axis === "col";
  const menu = el("div", "popmenu ctx-menu");
  menu.id = "sheet-ctx";
  const item = (label, danger, fn) => {
    const b = el("button", danger ? "danger" : null, label);
    b.addEventListener("click", () => { closeSheetMenu(); fn(); });
    menu.appendChild(b);
  };
  const sep = () => menu.appendChild(el("div", "menu-sep"));
  const span = () => {
    const r = effectiveRange();
    return { r, n: isCol ? r.c1 - r.c0 + 1 : r.r1 - r.r0 + 1 };
  };
  const what = isCol ? "column" : "row";
  const plural = (n) => (n === 1 ? what : `${what}s`);

  item("Cut", false, () => doCut());
  item("Copy", false, () => doCopy());
  item("Paste", false, () => doPaste());
  sep();
  const { n } = span();
  item(isCol ? `Insert ${n} ${plural(n)} left` : `Insert ${n} ${plural(n)} above`, false, () => {
    const { r, n: count } = span();
    tryEdit(() => (isCol
      ? wasm.session_insert_columns(state.sheet, r.c0, count)
      : wasm.session_insert_rows(state.sheet, r.r0, count)));
  });
  item(isCol ? `Insert ${n} ${plural(n)} right` : `Insert ${n} ${plural(n)} below`, false, () => {
    const { r, n: count } = span();
    tryEdit(() => (isCol
      ? wasm.session_insert_columns(state.sheet, r.c1 + 1, count)
      : wasm.session_insert_rows(state.sheet, r.r1 + 1, count)));
  });
  item(`Delete ${n} ${plural(n)}`, true, () => {
    const { r, n: count } = span();
    tryEdit(() => (isCol
      ? wasm.session_delete_columns(state.sheet, r.c0, count)
      : wasm.session_delete_rows(state.sheet, r.r0, count)));
  });
  item("Clear contents", false, () => clearSelection());
  sep();
  item(isCol ? "Column width…" : "Row height…", false, () => {
    const r = selRect();
    sizeDialog(axis, isCol ? r.c0 : r.r0);
  });
  item(isCol ? "Autofit width" : "Autofit height", false, () => {
    const r = selRect();
    if (isCol) for (let c = r.c0; c <= r.c1; c += 1) autofitColumn(c);
    else for (let row = r.r0; row <= r.r1; row += 1) autofitRow(row);
  });
  sep();
  item(`Hide ${plural(n)}`, false, () => {
    const { r } = span();
    tryEdit(() => (isCol
      ? wasm.session_hide_cols(state.sheet, r.c0, r.c1)
      : wasm.session_hide_rows(state.sheet, r.r0, r.r1)));
  });
  item("Unhide", false, () => {
    const { r } = span();
    tryEdit(() => (isCol
      ? wasm.session_unhide_cols(state.sheet, r.c0, r.c1)
      : wasm.session_unhide_rows(state.sheet, r.r0, r.r1)));
  });
  positionMenu(menu, x, y);
}

// Ctrl+D / Ctrl+R: fill the selection from its own first row / first column.
// The source is that edge, the destination is the rest of the block — which is
// exactly the drag-fill the handle performs, without the dragging.
function fillWithin(dir) {
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
  } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}

// Double-clicking the fill handle fills down to the extent of the neighbouring
// column's data — Excel's "finish this column" gesture, which beats dragging
// when the table is a thousand rows long. Uses the column to the left, falling
// back to the one on the right, as Excel does.
function autofillToNeighbour() {
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
  } catch (e) { status.textContent = `error: ${e}`; }
  draw();
}

// Live-update the drag-fill target box (extends the source in the dominant axis).
function updateFill(px, py) {
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

// Live-update the previewed size of the line being dragged.
function updateResize(px, py) {
  const f = state.freeze || { fc: 0, fr: 0 };
  if (state.resize.axis === "col") {
    // A frozen column's left edge ignores the scroll offset (mirror fscreenX).
    const off = wasm.session_col_offset_px(state.sheet, state.resize.index);
    const left = HW + off - (state.resize.index < f.fc ? 0 : state.scrollX);
    state.resize.previewPx = Math.max(MIN_LINE, Math.round(px - left));
  } else {
    const off = rowOffsetPx(state.resize.index);
    const top = HH + off - (state.resize.index < f.fr ? 0 : state.scrollY);
    state.resize.previewPx = Math.max(MIN_LINE, Math.round(py - top));
  }
  draw();
}

function wireEvents() {
  // Autofilter header buttons open on click, not mousedown — see the note in
  // the mousedown handler. stopPropagation keeps any already-armed
  // dismiss-on-next-click from closing the menu we are about to open.
  canvas.addEventListener("click", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    const fb = filterButtonAt(px, py);
    if (!fb) return;
    e.stopPropagation();
    openColumnFilter(
      fb.col,
      rect.left + fb.x * state.zoom,
      rect.top + (fb.y + fb.h) * state.zoom,
    );
  });

  canvas.addEventListener("mousedown", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    // While editing a formula at a reference position, clicking a cell inserts
    // its reference instead of moving the selection. preventDefault keeps the
    // inline input focused (no blur/commit); a drag turns it into a range.
    if (refAcceptable()) {
      const hit = cellAt(px, py);
      if (hit) {
        e.preventDefault();
        formulaRefDrag = { anchor: hit, start: editSurface.selectionStart, end: editSurface.selectionStart };
        insertRef(A1(hit.row, hit.col));
        hideAutocomplete();
        return;
      }
    }
    // An outline collapse toggle in the gutter.
    const ot = outlineToggleAt(px, py);
    if (ot) {
      tryEdit(() => wasm.session_toggle_outline(state.sheet, ot.index, ot.columns));
      return;
    }
    // A hidden-band handle in either header: one click brings the band back.
    // Double-click still works (below) for anyone who learned it that way.
    const hm = hiddenMarkAt(px, py);
    if (hm) { unhideMark(hm); return; }
    // An autofilter header button. Swallowed here so the press neither moves
    // the selection nor starts a drag; the menu itself opens on the `click`
    // that follows (see the canvas click handler) because `positionMenu` arms
    // its dismiss-on-next-click with a zero timeout — open from mousedown and
    // that timer fires before the click, so the click closes the menu on the
    // spot and it only appears while the button is held.
    if (filterButtonAt(px, py)) {
      if (editSurface && !commit(editSurface.value, false)) return;
      e.preventDefault();
      return;
    }
    // The active cell's data-validation dropdown button.
    if (validationChevron) {
      const c = validationChevron;
      if (px >= c.x && px <= c.x + c.w && py >= c.y && py <= c.y + c.h) {
        openValidationMenu(); canvas.focus(); return;
      }
    }
    // Any other click while editing first commits the in-progress edit (like
    // Excel) rather than discarding it. If the value is an invalid formula the
    // commit is refused and the click is swallowed so the user stays in the cell.
    if (editSurface && !commit(editSurface.value, false)) return;
    // The fill handle (bottom-right of the selection) starts a drag-fill.
    if (fillHandleRect && Math.abs(px - fillHandleRect.x) <= 5 && Math.abs(py - fillHandleRect.y) <= 5) {
      endInline();
      state.fill = { src: selRect(), dst: null };
      canvas.focus();
      return;
    }
    // Dragging the freeze divider (in the body) changes/removes the freeze.
    const fh = freezeHit(px, py);
    if (fh) { endInline(); state.freezeDrag = { axis: fh.axis, px, py }; return; }
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
    // Header clicks: select-all (corner), or a whole column/row — supporting
    // Shift-extend, Ctrl/Cmd multi-select (banking a range per addRange), and
    // drag-to-extend across adjacent headers (state.headerDrag drives both the
    // mousemove handler and edge auto-scroll below).
    if (px < HW && py < HH) { selectAll(); canvas.focus(); return; }
    if (py < HH && px >= HW) {
      endInline();
      const c = colAtX(px);
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey) addColumnRange(c);
      else selectColumn(c, e.shiftKey);
      state.headerDrag = "col";
      state.dragging = true;
      canvas.focus();
      return;
    }
    if (px < HW && py >= HH) {
      endInline();
      const r = rowAtY(py);
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey) addRowRange(r);
      else selectRow(r, e.shiftKey);
      state.headerDrag = "row";
      state.dragging = true;
      canvas.focus();
      return;
    }
    const hit = cellAt(px, py);
    if (hit) {
      endInline();
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey) { addRange(hit.row, hit.col); state.dragging = true; }
      else if (e.shiftKey) extend(hit.row, hit.col);
      else { select(hit.row, hit.col); state.dragging = true; }
      canvas.focus();
    }
  });
  canvas.addEventListener("mousemove", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    if (state.freezeDrag) { state.freezeDrag.px = px; state.freezeDrag.py = py; draw(); return; }
    if (state.resize) { updateResize(px, py); return; }
    if (state.fill) { updateFill(px, py); return; }
    if (formulaRefDrag) {
      const hit = cellAt(px, py);
      if (hit) {
        const a = formulaRefDrag.anchor;
        const r0 = Math.min(a.row, hit.row), r1 = Math.max(a.row, hit.row);
        const c0 = Math.min(a.col, hit.col), c1 = Math.max(a.col, hit.col);
        const ref = (r0 === r1 && c0 === c1) ? A1(r0, c0) : `${A1(r0, c0)}:${A1(r1, c1)}`;
        insertRef(ref);
      }
      return;
    }
    if (state.dragging && state.headerDrag) {
      // Dragging across column/row headers extends the whole-column/row
      // selection to the header under the pointer (anchor stays put).
      dragPos = { px, py };
      if (state.headerDrag === "col") {
        const c = colAtX(Math.max(HW + 1, px));
        if (c !== state.sel.col) selectColumn(c, true);
      } else {
        const r = rowAtY(Math.max(HH + 1, py));
        if (r !== state.sel.row) selectRow(r, true);
      }
      maybeAutoScroll();
      return;
    }
    if (state.dragging) {
      dragPos = { px, py };
      const f = state.freeze || { bodyX0: HW, bodyY0: HH };
      // Clamp into the body so a pointer over a header still maps to a cell.
      const cx = Math.min(Math.max(px, f.bodyX0 + 1), rect.width - 2);
      const cy = Math.min(Math.max(py, f.bodyY0 + 1), rect.height - 2);
      const hit = cellAt(cx, cy);
      // Set the focus directly (no ensureVisible — it would fight auto-scroll
      // and make the selection flicker/vanish). Anchor stays put.
      if (hit && (hit.row !== state.sel.row || hit.col !== state.sel.col)) {
        state.sel = { row: hit.row, col: hit.col };
        state.selKind = "cells";
        draw();
      }
      maybeAutoScroll();
      return;
    }
    // Idle hover: fill cursor over the handle, resize cursor over a boundary.
    if (fillHandleRect && Math.abs(px - fillHandleRect.x) <= 5 && Math.abs(py - fillHandleRect.y) <= 5) {
      canvas.style.cursor = "crosshair";
      return;
    }
    if (outlineToggleAt(px, py)) { canvas.style.cursor = "pointer"; return; }
    // A hidden-band handle: say what it does, since a collapsed band is the one
    // thing on the grid you cannot select your way out of.
    const hmh = hiddenMarkAt(px, py);
    if (hmh) {
      canvas.style.cursor = "pointer";
      const n = hmh.mark.to - hmh.mark.from + 1;
      const what = hmh.axis === "col" ? "column" : "row";
      status.textContent = `${n} hidden ${what}${n === 1 ? "" : "s"} — click to show`;
      return;
    }
    const fh = freezeHit(px, py);
    const hb = fh ? null : boundaryAt(px, py);
    canvas.style.cursor = (fh || hb)
      ? ((fh || hb).axis === "col" ? "col-resize" : "row-resize")
      : "cell";
    // Comment tooltip on hover.
    const hit = !hb && py >= HH && px >= HW ? cellAt(px, py) : null;
    if (hit && errorCells.has(hit.row + "," + hit.col)) {
      // Hovering an error explains it, and names the formula that produced it —
      // the two things you need before you can fix it.
      let code = "", input = "";
      try {
        code = JSON.parse(wasm.session_cells(state.sheet, hit.row, hit.col, hit.row, hit.col))[0]?.t || "";
        input = wasm.session_cell_input(state.sheet, hit.row, hit.col);
      } catch {}
      const help = ERROR_HELP[code] || "This formula could not be calculated.";
      commentTip.textContent = input.startsWith("=") ? `${code} — ${help}\n${input}` : `${code} — ${help}`;
      commentTip.style.whiteSpace = "pre-line";
      commentTip.style.left = (px + 14) + "px";
      commentTip.style.top = (py + 8) + "px";
      commentTip.hidden = false;
    } else if (hit && commentCells.has(hit.row + "," + hit.col)) {
      let text = "";
      try { text = wasm.session_comment_at(state.sheet, hit.row, hit.col); } catch {}
      if (text) {
        commentTip.textContent = text;
        commentTip.style.whiteSpace = "";
        commentTip.style.left = (px + 14) + "px";
        commentTip.style.top = (py + 8) + "px";
        commentTip.hidden = false;
      } else commentTip.hidden = true;
    } else {
      commentTip.hidden = true;
    }
  });
  window.addEventListener("mouseup", () => {
    if (state.freezeDrag) {
      const d = state.freezeDrag;
      state.freezeDrag = null;
      commitFreezeDrag(d.axis, d.px, d.py);
      renderTabs();
      draw();
      return;
    }
    if (formulaRefDrag) {
      const caret = formulaRefDrag.end;
      formulaRefDrag = null;
      const surface = editSurface ?? inline;
      surface.focus();
      surface.setSelectionRange(caret, caret);
      return;
    }
    if (state.resize) {
      const r = state.resize;
      state.resize = null;
      // A narrower column wraps to more lines, so its rows grow differently.
      invalidateGrowth();
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
    if (state.fill) {
      const f = state.fill;
      state.fill = null;
      const d = f.dst;
      if (d && (d.r0 !== f.src.r0 || d.c0 !== f.src.c0 || d.r1 !== f.src.r1 || d.c1 !== f.src.c1)) {
        try {
          wasm.session_fill(state.sheet, f.src.r0, f.src.c0, f.src.r1, f.src.c1, d.r0, d.c0, d.r1, d.c1);
          status.textContent = "filled";
        } catch (e) { status.textContent = `error: ${e}`; }
        state.anchor = { row: d.r0, col: d.c0 };
        state.sel = { row: d.r1, col: d.c1 };
        state.selKind = "cells";
      }
      draw();
    }
    const wasDragging = state.dragging || state.headerDrag;
    state.dragging = false;
    state.headerDrag = null;
    dragPos = null;
    stopAutoScroll();
    // The format painter paints whatever was just selected — after the drag
    // finishes, so brushing across a block applies to the whole block.
    if (painter && wasDragging) applyPainter(effectiveRange());
  });

  // Custom scrollbar thumb dragging.
  let sbDrag = null;
  const startThumb = (axis, el) => (e) => {
    e.preventDefault();
    const start = axis === "v" ? e.clientY : e.clientX;
    sbDrag = { axis, start, scroll0: axis === "v" ? state.scrollY : state.scrollX };
    el.classList.add("drag");
  };
  vthumb.addEventListener("mousedown", startThumb("v", vthumb));
  hthumb.addEventListener("mousedown", startThumb("h", hthumb));
  window.addEventListener("mousemove", (e) => {
    if (!sbDrag) return;
    if (sbDrag.axis === "v") {
      const d = e.clientY - sbDrag.start;
      state.scrollY = Math.max(0, sbDrag.scroll0 + (d / scrollMeta.vSpan) * scrollMeta.maxScrollY);
    } else {
      const d = e.clientX - sbDrag.start;
      state.scrollX = Math.max(0, sbDrag.scroll0 + (d / scrollMeta.hSpan) * scrollMeta.maxScrollX);
    }
    draw();
  });
  window.addEventListener("mouseup", () => {
    if (sbDrag) { vthumb.classList.remove("drag"); hthumb.classList.remove("drag"); sbDrag = null; }
  });
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom, py = (e.clientY - rect.top) / state.zoom;
    // A right-click in a header targets that row or column. cellAt() returns
    // null over the header strips, so this used to fall through and open the
    // cell menu against whatever was selected *before* — every verb in it then
    // acted on the wrong target, including Delete.
    if (HH && py < HH && px >= HW) {
      const col = colAtX(px);
      const r = selRect();
      const covered = state.selKind === "cols" && col >= r.c0 && col <= r.c1;
      if (!covered) selectColumn(col, false);
      headerMenu("col", e.clientX, e.clientY);
      return;
    }
    if (HW && px < HW && py >= HH) {
      const row = rowAtY(py);
      const r = selRect();
      const covered = state.selKind === "rows" && row >= r.r0 && row <= r.r1;
      if (!covered) selectRow(row, false);
      headerMenu("row", e.clientX, e.clientY);
      return;
    }
    if (HW && HH && px < HW && py < HH) { selectAll(); cellMenu(e.clientX, e.clientY); return; }
    const hit = cellAt(px, py);
    if (hit && state.selKind === "cells") {
      const r = selRect();
      const inside = hit.row >= r.r0 && hit.row <= r.r1 && hit.col >= r.c0 && hit.col <= r.c1;
      if (!inside) select(hit.row, hit.col);
    }
    cellMenu(e.clientX, e.clientY);
  });
  canvas.addEventListener("dblclick", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    // The fill handle: double-click fills down to the neighbouring column's
    // last row. Checked before the header/boundary cases because the handle can
    // sit anywhere in the body.
    if (fillHandleRect && Math.abs(px - fillHandleRect.x) <= 5 && Math.abs(py - fillHandleRect.y) <= 5) {
      autofillToNeighbour();
      return;
    }
    // Double-clicking a hidden-band handle unhides it too (single click already
    // did, on mousedown — this only catches the second click of a fast pair).
    const hmd = hiddenMarkAt(px, py);
    if (hmd) { unhideMark(hmd); return; }
    // Double-clicking a column or row boundary auto-fits it to its content.
    const hb = boundaryAt(px, py);
    if (hb) {
      if (hb.axis === "col") autofitColumn(hb.index);
      else autofitRow(hb.index);
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
      // Ctrl/⌘+wheel zooms, as everywhere else. The step is proportional so it
      // feels even at both ends of the range.
      if (e.ctrlKey || e.metaKey) {
        setZoom(state.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
        return;
      }
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
    // Alt+Down opens the active cell's validation dropdown (Excel parity).
    if (e.altKey && e.key === "ArrowDown" && validationChevron) { openValidationMenu(); e.preventDefault(); return; }
    const mod = e.ctrlKey || e.metaKey;

    // Keyboard shortcuts.
    if (mod) {
      // Ctrl+Arrow: jump to the data-edge (Excel block-jump).
      const arrow = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (arrow) {
        const to = JSON.parse(wasm.session_edge(state.sheet, state.sel.row, state.sel.col, arrow[0], arrow[1]));
        if (e.shiftKey) extend(to.row, to.col); else select(to.row, to.col);
        e.preventDefault(); return;
      }
      // Ctrl+PageDown / PageUp switch sheets (Excel parity).
      if (e.key === "PageDown") { const n = JSON.parse(wasm.session_sheet_names()).length; if (state.sheet < n - 1) switchSheet(state.sheet + 1); e.preventDefault(); return; }
      if (e.key === "PageUp") { if (state.sheet > 0) switchSheet(state.sheet - 1); e.preventDefault(); return; }
      const k = e.key.toLowerCase();
      if (k === "home") { select(0, 0); e.preventDefault(); return; }
      if (k === "end") { const b = usedBounds(); select(b.rows - 1, b.cols - 1); e.preventDefault(); return; }
      // Ctrl+D / Ctrl+R: fill the selection down from its top row / right from
      // its left column — the fastest way to copy a formula over a block.
      if (k === "d" && !e.shiftKey) { fillWithin("down"); e.preventDefault(); return; }
      if (k === "r" && !e.shiftKey) { fillWithin("right"); e.preventDefault(); return; }
      if (k === "0") { setZoom(1); e.preventDefault(); return; }
      if (k === "b") { toggleBold(); e.preventDefault(); return; }
      if (k === "i") { toggleItalic(); e.preventDefault(); return; }
      if (k === "u") { toggleUnderline(); e.preventDefault(); return; }
      if (e.shiftKey && (k === "7" || k === "&")) { toggleBorder(); e.preventDefault(); return; }
      if (e.shiftKey && (k === "l" || k === "e" || k === "r")) {
        setAlign(k === "l" ? "left" : k === "e" ? "center" : "right"); e.preventDefault(); return;
      }
      if (e.key === " ") { if (e.shiftKey) ctrlA(); else selectColsSpan(); e.preventDefault(); return; } // Ctrl+Space cols; Ctrl+Shift+Space all
      if (k === "a") { ctrlA(); e.preventDefault(); return; }
      if (k === "f") { openFind(); e.preventDefault(); return; }
      if (k === "g") { cellRef.focus(); e.preventDefault(); return; } // Go-To / Name box
      if (e.key === "F3") { const r = canvas.getBoundingClientRect(); openNameManager(r.left + 120, r.top + 90); e.preventDefault(); return; } // Name Manager
      if (k === "z" && !e.shiftKey) { doUndo(); e.preventDefault(); return; }
      if (k === "y" || (k === "z" && e.shiftKey)) { doRedo(); e.preventDefault(); return; }
      if (k === "s") { doSave(); e.preventDefault(); return; }
      if (k === "c") { await doCopy(); e.preventDefault(); return; }
      if (k === "x") { await doCut(); e.preventDefault(); return; }
      if (k === "v" && e.shiftKey) { doPasteMode("values"); e.preventDefault(); return; }
      if (k === "v") { await doPaste(); e.preventDefault(); return; }
      // Ctrl+Shift+"+" inserts rows/columns, Ctrl+"-" deletes them (Excel).
      // "+" arrives as key "+" or as "=" with Shift depending on the layout.
      if ((e.key === "+" || (k === "=" && e.shiftKey))) { insertLines(); e.preventDefault(); return; }
      if (e.key === "-" || e.key === "_") { deleteLines(); e.preventDefault(); return; }
    }

    const move = (dr, dc) => {
      if (e.shiftKey) extend(state.sel.row + dr, state.sel.col + dc);
      else select(state.sel.row + dr, state.sel.col + dc);
    };
    switch (e.key) {
      case "ArrowUp": move(-1, 0); e.preventDefault(); break;
      case "ArrowDown": move(1, 0); e.preventDefault(); break;
      case "Enter": select(state.sel.row + (e.shiftKey ? -1 : 1), state.sel.col); e.preventDefault(); break;
      case "ArrowLeft": move(0, -1); e.preventDefault(); break;
      case "ArrowRight": move(0, 1); e.preventDefault(); break;
      case "Tab": select(state.sel.row, state.sel.col + (e.shiftKey ? -1 : 1)); e.preventDefault(); break;
      case "Home": if (e.shiftKey) extend(state.sel.row, 0); else select(state.sel.row, 0); e.preventDefault(); break;
      case "End": { const ec = Math.max(0, usedBounds().cols - 1); if (e.shiftKey) extend(state.sel.row, ec); else select(state.sel.row, ec); e.preventDefault(); break; }
      case "PageDown": { const p = Math.max(1, geo.rows - 1); move(p, 0); e.preventDefault(); break; }
      case "PageUp": { const p = Math.max(1, geo.rows - 1); move(-p, 0); e.preventDefault(); break; }
      case "Backspace": case "Delete": clearSelection(); e.preventDefault(); break;
      case "F2": {
        if (e.shiftKey) openPanel("note"); // Shift+F2 → note (Excel parity)
        else startInline();
        e.preventDefault(); break;
      }
      case "F5": cellRef.focus(); e.preventDefault(); break;
      case " ": if (e.shiftKey) selectRowsSpan(); else startInline(" "); e.preventDefault(); break; // Shift+Space → whole rows
      case "Escape":
        if (painter) { setPainter(null); status.textContent = "format painter off"; e.preventDefault(); }
        else if (clipMarch) { stopMarch(); e.preventDefault(); }
        break;
      default:
        if (e.key.length === 1 && !mod) { startInline(e.key); e.preventDefault(); }
    }
  });
  // Both editing surfaces share one set of handlers: the in-cell editor and the
  // formula bar behave identically (autocomplete, Escape reverts, Enter commits
  // and advances, Tab commits and moves right), because they are two views of
  // the same edit session.
  for (const surface of [inline, fInput]) {
    surface.addEventListener("input", () => {
      // Typing in the formula bar with no edit open starts one.
      if (!editSurface) beginEdit(surface, surface.value);
      surface.classList.remove("invalid");
      // Typing anything ends point mode and leaves the reference it built in
      // place — a programmatic value change (insertRef) fires no input event,
      // so this only trips on real keystrokes.
      pointMode = null;
      mirrorEdit();
      showAutocomplete();
      updateSignatureTip();
      updateRefSpans();
    });
    // Moving the caret changes which argument you are in, without changing the
    // text — keyup and click are when the new position is readable.
    surface.addEventListener("keyup", (e) => {
      if (e.key.startsWith("Arrow") || e.key === "Home" || e.key === "End") updateSignatureTip();
    });
    surface.addEventListener("click", () => updateSignatureTip());
    surface.addEventListener("keydown", (e) => {
      // Autocomplete navigation takes priority when its menu is open.
      if (acState) {
        if (e.key === "ArrowDown") { acState.idx = (acState.idx + 1) % acState.matches.length; renderAutocomplete(); e.preventDefault(); return; }
        if (e.key === "ArrowUp") { acState.idx = (acState.idx - 1 + acState.matches.length) % acState.matches.length; renderAutocomplete(); e.preventDefault(); return; }
        if (e.key === "Enter" || e.key === "Tab") { acceptAutocomplete(); e.preventDefault(); return; }
        if (e.key === "Escape") { hideAutocomplete(); e.preventDefault(); return; }
      }
      // Arrow keys build a reference when the caret sits where one may go
      // (point mode); everywhere else they move the text caret as usual.
      const step = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (step && (pointMode || refAcceptable())) {
        if (pointStep(step[0], step[1], e.shiftKey)) { e.preventDefault(); return; }
      }
      if (e.key === "F4") { if (cycleAnchors()) e.preventDefault(); }
      // Alt+Enter breaks the line inside the cell instead of committing. Only
      // the in-cell editor can hold one — the formula bar is a single line.
      else if (e.key === "Enter" && e.altKey && surface === inline) {
        e.preventDefault();
        const at = surface.selectionStart;
        surface.value = surface.value.slice(0, at) + "\n" + surface.value.slice(surface.selectionEnd);
        surface.setSelectionRange(at + 1, at + 1);
        mirrorEdit();
        positionInline();
      }
      else if (e.key === "Enter") { commit(surface.value, true); e.preventDefault(); }
      else if (e.key === "Escape") { cancelEdit(); e.preventDefault(); }
      else if (e.key === "Tab") { if (commit(surface.value, false)) select(state.sel.row, state.sel.col + 1); e.preventDefault(); }
    });
  }
  // Clicking into the formula bar opens an edit on the active cell, so a click
  // there is the same gesture as F2 in the grid. Clicking it *during* an in-cell
  // edit hands that same edit over — the text is already mirrored, so only the
  // surface changes and the cell keeps showing what is typed.
  fInput.addEventListener("focus", () => {
    if (!editSurface) beginEdit(fInput);
    else if (editSurface === inline) editSurface = fInput;
  });
  // Leaving the formula bar commits, as Excel does. Two paths must not: handing
  // the edit back to the in-cell editor, and clicking a cell to pick a
  // reference (that path calls preventDefault, so no blur fires at all — the
  // formulaRefDrag guard is belt and braces).
  fInput.addEventListener("blur", (e) => {
    if (e.relatedTarget === inline || formulaRefDrag) return;
    if (editSurface === fInput) commit(fInput.value, false);
  });
  // Name box: Enter jumps to the typed cell/range; Escape reverts. Focus selects
  // all so it's ready to retype.
  cellRef.addEventListener("focus", () => cellRef.select());
  cellRef.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { gotoName(cellRef.value); canvas.focus(); updateNameBox(); e.preventDefault(); }
    else if (e.key === "Escape") { canvas.focus(); updateNameBox(); e.preventDefault(); }
  });

  // Find & replace bar.
  findInput.addEventListener("input", runFind);
  findInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { findStep(e.shiftKey ? -1 : 1); e.preventDefault(); }
    else if (e.key === "Escape") { closeFind(); e.preventDefault(); }
  });
  replaceInput.addEventListener("keydown", (e) => { if (e.key === "Escape") closeFind(); });
  for (const box of [findCase, findWhole, findValues, findAllSheets, findWildcards]) {
    box.addEventListener("change", runFind);
  }
  document.getElementById("find-next").addEventListener("click", () => findStep(1));
  document.getElementById("find-prev").addEventListener("click", () => findStep(-1));
  document.getElementById("replace-one").addEventListener("click", replaceOne);
  document.getElementById("replace-all").addEventListener("click", replaceAll);
  document.getElementById("find-close").addEventListener("click", closeFind);

  document.getElementById("hdr-open").addEventListener("click", () => document.getElementById("tb-open").click());

  // Popover menus: click toggles, outside-click / Escape closes, only one open.
  const menus = [];
  // The menus are `position: fixed` (to escape the toolbar's overflow clip), so
  // anchor each under its trigger button in viewport coordinates, flipping to
  // stay on-screen at the right and bottom edges.
  function anchorMenu(menu, btn) {
    const r = btn.getBoundingClientRect();
    menu.style.left = "0px";
    menu.style.top = "0px";
    const mw = menu.offsetWidth, mh = menu.offsetHeight;
    let left = r.left;
    let top = r.bottom + 4;
    if (left + mw > window.innerWidth - 4) left = Math.max(4, window.innerWidth - 4 - mw);
    if (top + mh > window.innerHeight - 4) top = Math.max(4, r.top - 4 - mh);
    menu.style.left = left + "px";
    menu.style.top = top + "px";
  }
  function wirePopup(btnId, menuId, onItem) {
    const btn = document.getElementById(btnId);
    const menu = document.getElementById(menuId);
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      menu.hidden = !open;
      if (!menu.hidden) anchorMenu(menu, btn);
    });
    for (const item of menu.querySelectorAll("button")) {
      item.addEventListener("click", () => { onItem(item); menu.hidden = true; canvas.focus(); });
    }
  }
  document.addEventListener("click", () => { for (const m of menus) m.hidden = true; });

  // Color popovers: custom toggle so the hex field doesn't close the menu;
  // rebuilt on each open so the Recent row stays current.
  for (const [btnId, menuId, onPick, noneLabel] of [
    ["tb-fontcolor", "fontcolor-menu", setFontColor, "Automatic"],
    ["tb-fillcolor", "fillcolor-menu", setFill, "No fill"],
  ]) {
    const btn = document.getElementById(btnId);
    const menu = document.getElementById(menuId);
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      if (open) { buildColorMenu(menu, onPick, noneLabel); menu.hidden = false; anchorMenu(menu, btn); }
    });
  }
  wirePopup("tb-numfmt", "numfmt-menu", (b) =>
    (b.dataset.nf === "__custom__" ? customFormatDialog() : setNumberFormat(b.dataset.nf)));
  // Border palette: custom toggle (its placement buttons apply and close, but
  // the style select and color swatches must not), built once here.
  buildBorderMenu();
  // The palette is built once, so its "current pick" marks would go stale the
  // moment anything changed them. Re-sync on every open, and mirror the colour
  // onto the toolbar button so the choice is visible without opening the menu.
  function syncBorderPicks() {
    const menu = document.getElementById("border-menu");
    for (const sw of menu.querySelectorAll(".bd-color")) {
      sw.classList.toggle("on", (sw.dataset.color || "") === borderColor);
    }
    const sel = menu.querySelector(".bd-style");
    if (sel) sel.value = borderStyle;
    const btn = document.getElementById("tb-border");
    btn.style.setProperty("--bd-color", borderColor ? "#" + borderColor : "currentColor");
  }
  {
    const btn = document.getElementById("tb-border");
    const menu = document.getElementById("border-menu");
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      menu.hidden = !open;
      if (!menu.hidden) { syncBorderPicks(); anchorMenu(menu, btn); }
    });
    syncBorderPicks();
  }
  // Styled tooltips over the chrome (converts native titles, incl. the border
  // palette just built above).
  initTooltips();
  wirePopup("tb-wrap", "wrap-menu", (b) => setTextOverflow(b.dataset.ov));
  wirePopup("tb-merge", "merge-menu", (b) => mergeVariant(b.dataset.mg));
  wirePopup("tb-rotate", "rotate-menu", (b) => setRotation(+b.dataset.rot));
  wirePopup("tb-valign", "valign-menu", (b) => setValign(b.dataset.va));
  wirePopup("tb-freeze", "freeze-menu", (b) => setFreeze(b.dataset.fz));
  wirePopup("tb-sort", "sort-menu", (b) =>
    (b.dataset.sort === "custom" ? sortDialog() : sortRange(b.dataset.sort === "desc")));
  document.getElementById("tb-filter").addEventListener("click", (e) => {
    e.stopPropagation();
    toggleFilter();
  });
  // Tool side panel: toolbar buttons toggle it; the header ✕ and Esc close it.
  document.getElementById("tb-dv").addEventListener("click", () => togglePanel("dv"));
  document.getElementById("tb-cf").addEventListener("click", () => togglePanel("cf"));
  document.getElementById("tb-note").addEventListener("click", () => togglePanel("note"));
  document.getElementById("side-panel-close").addEventListener("click", () => closePanel());

  document.getElementById("tb-size-up").addEventListener("click", () => { stepFontSize(1); canvas.focus(); });
  document.getElementById("tb-size-down").addEventListener("click", () => { stepFontSize(-1); canvas.focus(); });
  document.getElementById("tb-bold").addEventListener("click", () => { toggleBold(); canvas.focus(); });
  document.getElementById("tb-italic").addEventListener("click", () => { toggleItalic(); canvas.focus(); });
  document.getElementById("tb-underline").addEventListener("click", () => { toggleUnderline(); canvas.focus(); });
  document.getElementById("tb-strike").addEventListener("click", () => { toggleStrike(); canvas.focus(); });
  document.getElementById("tb-indent-more").addEventListener("click", () => { setIndent(1); canvas.focus(); });
  document.getElementById("tb-indent-less").addEventListener("click", () => { setIndent(-1); canvas.focus(); });
  {
    const pb = document.getElementById("tb-painter");
    pb.addEventListener("click", () => { painter ? setPainter(null) : armPainter(false); canvas.focus(); });
    pb.addEventListener("dblclick", () => { armPainter(true); canvas.focus(); });
  }
  document.getElementById("tb-currency").addEventListener("click", () => { setNumberFormat("$#,##0.00"); canvas.focus(); });
  document.getElementById("tb-percent").addEventListener("click", () => { setNumberFormat("0%"); canvas.focus(); });
  document.getElementById("tb-comma").addEventListener("click", () => { setNumberFormat("#,##0.00"); canvas.focus(); });
  document.getElementById("tb-inc-dec").addEventListener("click", () => { adjustDecimals(1); canvas.focus(); });
  document.getElementById("tb-dec-dec").addEventListener("click", () => { adjustDecimals(-1); canvas.focus(); });
  for (const b of document.querySelectorAll(".tb-align")) {
    b.addEventListener("click", () => { setAlign(b.dataset.al); canvas.focus(); });
  }
  // --- Font family / font size comboboxes ----------------------------------
  // Editable text box + a custom dropdown. NOT a native <input list=datalist>:
  // Chrome filters datalist options by the input's current text, so as soon as
  // the box mirrors the selected cell's font the dropdown collapsed to that one
  // entry ("only Calibri, no other fonts"). This list always opens in full,
  // narrows as you type, and still accepts any typed value.
  //
  // `values`: [{v, label, title}] — v is what gets applied, label what is shown.
  // `apply(v)`: commits the value. `preview`: render each row in its own face.
  function wireCombo({ input, caret, menu, values, apply, preview }) {
    menus.push(menu);
    const wrap = input.parentElement;
    let rows = [];       // the currently rendered option buttons
    let active = -1;     // index into rows of the highlighted option
    let committed = "";  // last value applied, so blur doesn't re-apply it

    const isOpen = () => !menu.hidden;
    function close() {
      menu.hidden = true;
      input.setAttribute("aria-expanded", "false");
      active = -1;
    }
    function highlight(i) {
      if (!rows.length) return;
      active = (i + rows.length) % rows.length;
      rows.forEach((b, n) => b.classList.toggle("active", n === active));
      rows[active].scrollIntoView({ block: "nearest" });
    }
    function choose(v) {
      input.value = v;
      committed = v;
      close();
      apply(v);
      canvas.focus();
    }
    // Build the option rows, narrowed to `filter` (a substring match, as the
    // box doubles as the search field). Returns how many rows are showing.
    function build(filter) {
      const f = (filter || "").trim().toLowerCase();
      const all = typeof values === "function" ? values() : values; // lazy: engine-sourced lists
      const list = f ? all.filter((o) => o.label.toLowerCase().includes(f)) : all;
      const cur = input.value.trim().toLowerCase();
      menu.textContent = "";
      active = -1;
      rows = list.map((o, i) => {
        const b = document.createElement("button");
        b.type = "button";
        b.className = "combo-item";
        b.setAttribute("role", "option");
        b.textContent = o.label;
        b.dataset.v = o.v; // label ≠ value for "Default" (which applies "")
        if (o.title) b.title = o.title;
        if (preview) b.style.fontFamily = fontStack(o.v);
        if (o.v.toLowerCase() === cur) { b.classList.add("checked"); active = i; }
        // mousedown must not blur the input (blur would close the menu first).
        b.addEventListener("mousedown", (e) => e.preventDefault());
        b.addEventListener("click", (e) => { e.stopPropagation(); choose(o.v); });
        menu.appendChild(b);
        return b;
      });
      return rows.length;
    }
    function open(filter) {
      for (const m of menus) if (m !== menu) m.hidden = true;
      if (!build(filter)) { close(); return; }
      menu.hidden = false;
      input.setAttribute("aria-expanded", "true");
      menu.style.minWidth = wrap.offsetWidth + "px";
      anchorMenu(menu, wrap);
      highlight(active < 0 ? 0 : active);
    }

    // The shared outside-click handler only sets `hidden`, which would leave
    // aria-expanded stale — close() properly on any click that isn't ours (the
    // caret and the option rows both stopPropagation).
    document.addEventListener("click", close);
    caret.addEventListener("mousedown", (e) => e.preventDefault()); // keep focus
    caret.addEventListener("click", (e) => {
      e.stopPropagation();
      if (isOpen()) { close(); return; }
      input.focus();
      open(""); // the caret always shows the whole list, never a filtered one
    });
    input.addEventListener("focus", () => { committed = input.value; input.select(); });
    input.addEventListener("input", () => open(input.value));
    input.addEventListener("blur", () => {
      close();
      // Typed-then-clicked-away still commits (the old <input> "change" path),
      // but only when the text actually changed — otherwise every focus pass
      // would push a redundant undo entry.
      if (input.value.trim() !== committed.trim()) { committed = input.value; apply(input.value.trim()); }
    });
    input.addEventListener("keydown", (e) => {
      e.stopPropagation(); // the grid's global handler must not see these keys
      if (e.key === "ArrowDown") { isOpen() ? highlight(active + 1) : open(""); e.preventDefault(); }
      else if (e.key === "ArrowUp") { if (isOpen()) { highlight(active - 1); e.preventDefault(); } }
      else if (e.key === "Enter") {
        e.preventDefault();
        if (isOpen() && rows[active]) choose(rows[active].dataset.v);
        else { committed = input.value; close(); apply(input.value.trim()); canvas.focus(); }
      } else if (e.key === "Escape") {
        e.preventDefault();
        // First Escape closes the list, a second reverts the box to the cell.
        // Order matters: mark the typed text as "committed" so the blur below
        // doesn't apply it, then blur (refreshFormulaBar skips a focused
        // control), then let the refresh restore the cell's real value.
        if (isOpen()) close();
        else { committed = input.value; canvas.focus(); refreshFormulaBar(); }
      } else if (e.key === "Tab") close();
    });
    return { close };
  }

  // Font family: every family the engine renders faithfully, each row drawn in
  // its own face. Substituted families say so in their tooltip rather than
  // pretending to be the real thing.
  const kindNote = { exact: "", metric: " — renders as %s (metric-compatible)", generic: " — renders as %s (closest match)" };
  wireCombo({
    input: document.getElementById("tb-font"),
    caret: document.getElementById("tb-font-caret"),
    menu: document.getElementById("font-menu"),
    values: () => [{ v: "", label: "Default", title: "Clear the font (use the workbook default)" }].concat(
      fontFamilies().map((f) => ({
        v: f.n,
        label: f.n,
        title: f.n + (kindNote[f.k] || "").replace("%s", f.f),
      }))),
    apply: (v) => setFontName(v.trim()),
    preview: true,
  });
  // Font size: the Excel ladder, but any typed size is accepted and clamped to
  // Excel's 1–409 pt range; a blank/zero clears the explicit size.
  wireCombo({
    input: document.getElementById("tb-size"),
    caret: document.getElementById("tb-size-caret"),
    menu: document.getElementById("size-menu"),
    values: [{ v: "", label: "Default", title: "Clear the size (use the workbook default)" }]
      .concat(SIZE_LADDER.map((n) => ({ v: String(n), label: String(n) }))), // same ladder as A▲/A▼
    apply: (v) => {
      const raw = parseFloat(v);
      setFontSize(Number.isFinite(raw) && raw > 0 ? Math.min(409, Math.max(1, raw)) : 0);
    },
  });
  document.getElementById("tb-open").addEventListener("change", async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const ext = (file.name.split(".").pop() || "").toLowerCase();
    // Delimiter byte by extension: tab=9, pipe=124, comma=44 (null → .xlsx).
    const delim = ext === "tsv" || ext === "tab" ? 9 : ext === "psv" ? 124 : ext === "csv" ? 44 : null;
    try {
      stopMarch();
      if (delim !== null) wasm.session_open_delimited(bytes, delim);
      else wasm.session_open(bytes);
      status.textContent = "opened " + file.name;
    } catch (err) { status.textContent = `error: ${err}`; }
    e.target.value = ""; // allow re-opening the same file
    invalidateGrowth();
    state.sheet = 0;
    state.scrollX = state.scrollY = 0;
    renderTabs();
    select(0, 0);
  });
  document.getElementById("tb-undo").addEventListener("click", doUndo);
  document.getElementById("tb-redo").addEventListener("click", doRedo);

  // --- Progressive toolbar collapse (Excel-ribbon style) ---
  // Each group tagged data-collapse=<priority> collapses in-place into its
  // "Label ▾" button — whose flyout holds the group's live tools — whenever the
  // single toolbar row would overflow, lowest priority first. Groups re-expand
  // as the window widens. Never a scrollbar, never a second row.
  const toolbarEl = document.querySelector(".toolbar");
  const collapsibles = [...toolbarEl.querySelectorAll(".tb-group[data-collapse]")]
    .map((groupEl) => ({
      groupEl,
      btn: toolbarEl.querySelector(`.tb-collapsed[data-for="${groupEl.id}"]`),
      flyout: toolbarEl.querySelector(`.tb-flyout[data-flyout="${groupEl.id}"]`),
      prio: +groupEl.dataset.collapse,
    }))
    .sort((a, b) => a.prio - b.prio); // lowest priority number collapses first
  const flyouts = collapsibles.map((c) => c.flyout);
  const closeFlyouts = () => { for (const f of flyouts) f.hidden = true; };
  const expandGroup = (c) => {
    while (c.flyout.firstChild) c.groupEl.appendChild(c.flyout.firstChild);
    c.groupEl.hidden = false; c.btn.hidden = true; c.flyout.hidden = true;
  };
  const collapseGroup = (c) => {
    while (c.groupEl.firstChild) c.flyout.appendChild(c.groupEl.firstChild);
    c.groupEl.hidden = true; c.btn.hidden = false;
  };
  const fits = () => toolbarEl.scrollWidth <= toolbarEl.clientWidth + 1;
  function reflowToolbar() {
    for (const c of collapsibles) expandGroup(c); // reset to fully expanded
    for (const c of collapsibles) { if (fits()) break; collapseGroup(c); }
  }
  for (const c of collapsibles) {
    c.btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = c.flyout.hidden;
      for (const m of menus) m.hidden = true;
      closeFlyouts();
      c.flyout.hidden = !open;
      if (!c.flyout.hidden) anchorMenu(c.flyout, c.btn);
    });
  }
  // A click *inside* a flyout must not dismiss it — the flyout holds the live
  // controls of the collapsed group (the font box, color swatches, …), and
  // clicking into one would otherwise close the panel out from under it.
  document.addEventListener("click", (e) => { if (!e.target.closest(".tb-flyout")) closeFlyouts(); });
  reflowToolbar();

  // --- Toolbar as one composite control (roving tabindex) ------------------
  // Every button and field in the toolbar was its own tab stop, so reaching the
  // grid by keyboard meant ~40 presses of Tab. A toolbar is one stop: Tab moves
  // past it, arrows move within it. Re-scanned on each interaction because the
  // collapse machinery moves controls in and out of flyouts.
  {
    const toolbar = document.querySelector(".toolbar");
    const items = () => [...toolbar.querySelectorAll("button, input, select")]
      .filter((el) => !el.disabled && el.offsetParent !== null);
    const rove = (list, at) => {
      for (const el of list) el.tabIndex = -1;
      const n = list.length;
      const target = list[((at % n) + n) % n];
      target.tabIndex = 0;
      target.focus();
    };
    const syncStops = () => {
      // Reset *every* control, including those parked in a collapsed group's
      // flyout: they are invisible now, but they keep their tabindex and would
      // become extra tab stops the moment the flyout opened.
      for (const el of toolbar.querySelectorAll("button, input, select")) el.tabIndex = -1;
      const list = items();
      if (!list.length) return;
      const focused = list.findIndex((el) => el === document.activeElement);
      list[Math.max(0, focused)].tabIndex = 0;
    };
    syncStops();
    toolbar.addEventListener("focusin", syncStops);
    toolbar.addEventListener("keydown", (e) => {
      // A text field owns its own arrow keys (the font and size boxes), so only
      // step between controls when the caret is not in one.
      const inField = document.activeElement && document.activeElement.tagName === "INPUT";
      if (inField && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;
      const list = items();
      const at = list.indexOf(document.activeElement);
      if (at < 0) return;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") { rove(list, at + 1); e.preventDefault(); }
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp") { rove(list, at - 1); e.preventDefault(); }
      else if (e.key === "Home") { rove(list, 0); e.preventDefault(); }
      else if (e.key === "End") { rove(list, list.length - 1); e.preventDefault(); }
    });
    // Re-establish a single stop after the toolbar reflows into/out of flyouts.
    window.addEventListener("resize", syncStops);
  }

  // Track whether the user is driving by keyboard, so the grid's focus ring
  // appears for them and not on every mouse click (see the CSS note).
  window.addEventListener("keydown", (e) => {
    if (e.key === "Tab" || e.key.startsWith("Arrow")) document.documentElement.dataset.kbnav = "1";
  }, true);
  window.addEventListener("mousedown", () => { delete document.documentElement.dataset.kbnav; }, true);

  buildMenuBar();

  // The page-header collapse toggle, kept right-most: buildMenuBar() appends
  // File…Help, so re-append it afterwards rather than relying on markup order.
  {
    const btn = document.getElementById("hdr-collapse");
    const bar = document.getElementById("menubar");
    if (btn && bar) {
      bar.appendChild(btn);
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        setHeaderCollapsed(!headerCollapsed);
      });
      let saved = false;
      try { saved = localStorage.getItem(HEADER_COLLAPSE_KEY) === "1"; } catch {}
      if (saved) setHeaderCollapsed(true);
    }
  }

  window.addEventListener("resize", () => { resize(); reflowToolbar(); });

  // ---- Application menu bar (File / Edit / View / …) -----------------------
  // Declarative menu → item table. Every item delegates to an existing handler
  // or toolbar control, so there are no parallel implementations to drift.
  function buildMenuBar() {
    const bar = document.getElementById("menubar");
    if (!bar) return;
    const q = (sel) => document.querySelector(sel);
    const clickEl = (sel) => () => { const n = q(sel); if (n) n.click(); };
    const rng = () => effectiveRange();
    const fmtHas = (k) => {
      try { return !!JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col))[k]; }
      catch { return false; }
    };
    const gridOn = () => { try { return !wasm.session_gridlines_hidden(state.sheet); } catch { return true; } };
    const headersOn = () => { try { return !wasm.session_headers_hidden(state.sheet); } catch { return true; } };
    const clearContents = () => {
      try { for (const s of allRanges()) wasm.session_clear_contents(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
      draw();
    };
    const nf = (code) => () => setNumberFormat(code);

    const showModal = (title, html) => {
      document.getElementById("oc-modal-title").textContent = title;
      document.getElementById("oc-modal-body").innerHTML = html;
      document.getElementById("oc-modal").hidden = false;
    };
    function showShortcuts() {
      const rows = [
        ["Undo / Redo", "Ctrl+Z / Ctrl+Shift+Z"],
        ["Cut / Copy / Paste", "Ctrl+X / Ctrl+C / Ctrl+V"],
        ["Bold / Italic / Underline", "Ctrl+B / Ctrl+I / Ctrl+U"],
        ["Find & replace", "Ctrl+F"],
        ["Select all", "Ctrl+A"],
        ["Insert / delete line", "Ctrl++ / Ctrl+−"],
        ["Edit cell", "F2 / Enter"],
        ["Name manager", "F3"],
      ];
      showModal("Keyboard shortcuts", rows.map(([a, b]) =>
        `<div class="kb-row"><span>${a}</span><span>${b.replace(/(\S+)/g, "<kbd>$1</kbd>").replace(/<kbd>\/<\/kbd>/g, "/")}</span></div>`).join(""));
    }
    function showAbout() {
      showModal("About OpenCalc",
        `<p>OpenCalc — a deterministic, embeddable spreadsheet engine for <code>.xlsx</code>, CSV, TSV and PSV.</p>
         <p style="margin-top:10px;color:var(--muted)">Engine <b>v0.0.0</b> · Alpha · <a href="./index.html">Home</a></p>`);
    }

    const MENUS = [
      ["File", [
        ["New", () => { stopMarch(); wasm.session_new(); state.sheet = 0; seed(); renderTabs(); }],
        ["Open…", clickEl("#tb-open")],
        { sub: "Download", items: [
          ["Excel (.xlsx)", () => saveAs("xlsx")],
          ["CSV (.csv)", () => saveAs("csv")],
          ["Tab-separated (.tsv)", () => saveAs("tsv")],
          ["Pipe-separated (.psv)", () => saveAs("psv")],
        ] },
      ]],
      ["Edit", [
        ["Undo", doUndo, "Ctrl+Z"],
        ["Redo", doRedo, "Ctrl+Shift+Z"],
        "sep",
        ["Cut", doCut, "Ctrl+X"],
        ["Copy", doCopy, "Ctrl+C"],
        ["Paste", doPaste, "Ctrl+V"],
        "sep",
        ["Find & replace…", openFind, "Ctrl+F"],
        ["Select all", selectAll, "Ctrl+A"],
        "sep",
        { sub: "Clear", items: [
          ["Values", clearContents],
          ["Formatting", clearFormats],
          ["All", clearAll],
        ] },
      ]],
      ["View", [
        { sub: "Freeze", items: [
          ["Up to selection", clickEl('#freeze-menu [data-fz="sel"]')],
          ["Top row", clickEl('#freeze-menu [data-fz="row"]')],
          ["First column", clickEl('#freeze-menu [data-fz="col"]')],
          ["Unfreeze", clickEl('#freeze-menu [data-fz="none"]')],
        ] },
        ["Gridlines", () => { try { wasm.session_set_gridlines_hidden(state.sheet, gridOn()); } catch {} draw(); }, null, gridOn],
        // "Cell markings" = the A/B/C and 1/2/3 strips. Deliberately not called
        // "headers": that word belongs to the page header this menu bar can
        // collapse, and having both under one name is a coin-flip every time.
        ["Cell markings", () => { try { wasm.session_set_headers_hidden(state.sheet, headersOn()); } catch {} resize(); }, null, headersOn],
        { sub: "Zoom", items: [
          ["50%", () => setZoom(0.5), null, () => state.zoom === 0.5],
          ["75%", () => setZoom(0.75), null, () => state.zoom === 0.75],
          ["100%", () => setZoom(1), "Ctrl+0", () => state.zoom === 1],
          ["150%", () => setZoom(1.5), null, () => state.zoom === 1.5],
          ["200%", () => setZoom(2), null, () => state.zoom === 2],
        ] },
        "sep",
        ["Settings…", () => { setHeaderCollapsed(false); clickEl("#tb-settings")(); }],
      ]],
      ["Insert", [
        ["Rows above", () => tryEdit(() => { const r = rng(); wasm.session_insert_rows(state.sheet, r.r0, r.r1 - r.r0 + 1); })],
        ["Rows below", () => tryEdit(() => { const r = rng(); wasm.session_insert_rows(state.sheet, r.r1 + 1, r.r1 - r.r0 + 1); })],
        ["Columns left", () => tryEdit(() => { const r = rng(); wasm.session_insert_columns(state.sheet, r.c0, r.c1 - r.c0 + 1); })],
        ["Columns right", () => tryEdit(() => { const r = rng(); wasm.session_insert_columns(state.sheet, r.c1 + 1, r.c1 - r.c0 + 1); })],
        "sep",
        ["Delete rows", () => tryEdit(() => { const r = rng(); wasm.session_delete_rows(state.sheet, r.r0, r.r1 - r.r0 + 1); })],
        ["Delete columns", () => tryEdit(() => { const r = rng(); wasm.session_delete_columns(state.sheet, r.c0, r.c1 - r.c0 + 1); })],
        "sep",
        ["Note", clickEl("#tb-note")],
      ]],
      ["Format", [
        ["Bold", clickEl("#tb-bold"), "Ctrl+B", () => fmtHas("b")],
        ["Italic", clickEl("#tb-italic"), "Ctrl+I", () => fmtHas("i")],
        ["Underline", clickEl("#tb-underline"), "Ctrl+U", () => fmtHas("u")],
        ["Strikethrough", clickEl("#tb-strike"), null, () => fmtHas("st")],
        "sep",
        ["Cell styles…", () => cellStyleGallery()],
        ["Conditional formatting rules…", () => manageCfRules()],
        { sub: "Alignment", items: [
          ["Left", () => setAlign("left")],
          ["Center", () => setAlign("center")],
          ["Right", () => setAlign("right")],
          // The OOXML modes that are more than an edge. `centerContinuous` is
          // Excel's "Center Across Selection" — it looks merged but merges
          // nothing, so the cells underneath stay addressable.
          ["Fill (repeat text)", () => setAlign("fill")],
          ["Justify", () => setAlign("justify")],
          ["Center across selection", () => setAlign("centerContinuous")],
          ["Distributed", () => setAlign("distributed")],
          ["Clear (General)", () => setAlign("")],
          "sep",
          ["Top", () => setValign("top")],
          ["Middle", () => setValign("middle")],
          ["Bottom", () => setValign("bottom")],
          ["Justify (vertical)", () => setValign("justify")],
          ["Distributed (vertical)", () => setValign("distributed")],
        ] },
        { sub: "Text overflow", items: [
          ["Overflow", () => setTextOverflow("overflow"), null, () => !fmtHas("w") && !fmtHas("cl")],
          ["Wrap", () => setTextOverflow("wrap"), null, () => fmtHas("w")],
          ["Clip", () => setTextOverflow("clip"), null, () => fmtHas("cl")],
        ] },
        ["Merge cells", clickEl("#tb-merge")],
        "sep",
        { sub: "Number", items: [
          ["Automatic", nf("")],
          ["Number (0.00)", nf("0.00")],
          ["Thousands (#,##0)", nf("#,##0")],
          ["Percent (0%)", nf("0%")],
          ["Currency", nf("$#,##0.00")],
          ["Short date", nf("yyyy-mm-dd")],
          ["Time", nf("h:mm:ss AM/PM")],
          ["Scientific", nf("0.00E+00")],
          ["Text", nf("@")],
        ] },
        ["Custom number format…", () => customFormatDialog()],
        ["Conditional formatting…", clickEl("#tb-cf")],
        "sep",
        ["Clear formatting", clearFormats],
      ]],
      ["Data", [
        { sub: "Sort range", items: [
          ["A → Z", clickEl('#sort-menu [data-sort="asc"]')],
          ["Z → A", clickEl('#sort-menu [data-sort="desc"]')],
          ["Custom sort…", () => sortDialog()],
        ] },
        ["Remove duplicates…", () => removeDuplicates()],
        ["Filter", () => toggleFilter()],
        ["Clear all filters", () => { if (!filterInfo) { status.textContent = "no filter"; return; } tryEdit(() => wasm.session_clear_filter_rules(state.sheet)); afterFilterChange(); }],
        ["Data validation…", clickEl("#tb-dv")],
        "sep",
        ["Hide rows", () => tryEdit(() => { const r = rng(); wasm.session_hide_rows(state.sheet, r.r0, r.r1); })],
        ["Hide columns", () => tryEdit(() => { const r = rng(); wasm.session_hide_cols(state.sheet, r.c0, r.c1); })],
        { sub: "Group", items: [
          ["Group rows", () => tryEdit(() => { const r = effectiveRange(); wasm.session_group(state.sheet, r.r0, r.r1, false); })],
          ["Group columns", () => tryEdit(() => { const r = effectiveRange(); wasm.session_group(state.sheet, r.c0, r.c1, true); })],
          ["Ungroup rows", () => tryEdit(() => { const r = effectiveRange(); wasm.session_ungroup(state.sheet, r.r0, r.r1, false); })],
          ["Ungroup columns", () => tryEdit(() => { const r = effectiveRange(); wasm.session_ungroup(state.sheet, r.c0, r.c1, true); })],
          "sep",
          ["Expand all", () => tryEdit(() => { wasm.session_show_outline_level(state.sheet, 7, false); wasm.session_show_outline_level(state.sheet, 7, true); })],
          ["Collapse all", () => tryEdit(() => { wasm.session_show_outline_level(state.sheet, 0, false); wasm.session_show_outline_level(state.sheet, 0, true); })],
          ["Show level 1", () => tryEdit(() => wasm.session_show_outline_level(state.sheet, 1, false))],
          ["Show level 2", () => tryEdit(() => wasm.session_show_outline_level(state.sheet, 2, false))],
        ] },
        "sep",
        ["Unhide rows/columns in selection", () => tryEdit(() => { const r = rng(); wasm.session_unhide_rows(state.sheet, r.r0, r.r1); wasm.session_unhide_cols(state.sheet, r.c0, r.c1); })],
        // Unhide-in-selection cannot help when you no longer know where the
        // hidden band is — or when it is outside whatever is selected. This
        // always works.
        ["Unhide all rows and columns", () => { const b = usedBounds(); tryEdit(() => { wasm.session_unhide_rows(state.sheet, 0, Math.max(b.rows, 1) + 1000); wasm.session_unhide_cols(state.sheet, 0, Math.max(b.cols, 1) + 1000); }); status.textContent = "all rows and columns shown"; }],
      ]],
      ["Tools", [
        ["Settings…", () => { setHeaderCollapsed(false); clickEl("#tb-settings")(); }],
        ["Name manager…", () => openNameManager(160, 120)],
      ]],
      ["Help", [
        ["Keyboard shortcuts", showShortcuts],
        ["About OpenCalc", showAbout],
      ]],
    ];

    const drops = [];
    const subs = [];
    const topBtns = [];
    let openIdx = -1;
    const closeSubs = () => { for (const s of subs) s.hidden = true; };
    const closeMenus = () => {
      for (const d of drops) d.hidden = true;
      closeSubs();
      for (const b of topBtns) b.setAttribute("aria-expanded", "false");
      openIdx = -1;
    };
    const positionSub = (sub, btn) => {
      const r = btn.getBoundingClientRect();
      sub.style.left = "0px"; sub.style.top = "0px";
      const sw = sub.offsetWidth, sh = sub.offsetHeight;
      let left = r.right - 3, top = r.top - 5;
      if (left + sw > window.innerWidth - 4) left = Math.max(4, r.left - sw + 3);
      if (top + sh > window.innerHeight - 4) top = Math.max(4, window.innerHeight - 4 - sh);
      sub.style.left = left + "px"; sub.style.top = top + "px";
    };
    const runItem = (action) => {
      try { action && action(); } catch (err) { status.textContent = `error: ${err}`; }
      closeMenus();
    };
    const renderItems = (container, items, isTop) => {
      for (const it of items) {
        if (it === "sep") {
          const s = document.createElement("div"); s.className = "menu-sep"; container.appendChild(s); continue;
        }
        if (it.sub) {
          const b = document.createElement("button");
          b.innerHTML = `<span class="mi-check"></span><span class="mi-label"></span><span class="mi-caret">&#9656;</span>`;
          b.querySelector(".mi-label").textContent = it.sub;
          const sub = document.createElement("div"); sub.className = "menu-sub popmenu"; sub.hidden = true;
          document.body.appendChild(sub); subs.push(sub);
          renderItems(sub, it.items, false);
          const openSub = () => { closeSubs(); positionSub(sub, b); sub.hidden = false; };
          b.addEventListener("mouseenter", openSub);
          b.addEventListener("click", (e) => { e.stopPropagation(); openSub(); });
          container.appendChild(b); continue;
        }
        const [label, action, key, check] = it;
        const b = document.createElement("button");
        b.innerHTML = `<span class="mi-check"></span><span class="mi-label"></span>${key ? `<span class="mi-key">${key}</span>` : ""}`;
        b.querySelector(".mi-label").textContent = label;
        if (check) b._check = check;
        if (isTop) b.addEventListener("mouseenter", closeSubs);
        b.addEventListener("click", (e) => { e.stopPropagation(); runItem(action); });
        container.appendChild(b);
      }
    };

    const openMenu = (i) => {
      closeMenus();
      for (const m of menus) m.hidden = true; // close toolbar popovers
      closeFlyouts();
      const drop = drops[i];
      // refresh checkmarks against the focus cell / view state
      for (const b of drop.querySelectorAll("button")) {
        if (b._check) b.querySelector(".mi-check").textContent = b._check() ? "✓" : "";
      }
      drop.hidden = false;
      anchorMenu(drop, topBtns[i]);
      topBtns[i].setAttribute("aria-expanded", "true");
      openIdx = i;
    };

    // Alt mnemonics: the first letter of each top-level menu, which is unique
    // across File/Edit/View/Insert/Format/Data/Tools/Help. Underlined only while
    // Alt is held, as on Windows — a permanently underlined letter reads as a
    // link, and on a Mac Alt is a compose key, so the hint stays out of the way
    // until it is relevant.
    const mnemonics = new Map();
    MENUS.forEach(([name, items], i) => {
      const btn = document.createElement("button");
      btn.className = "menu-top";
      // Not always the first letter: File and Format both start with F, so the
      // first-letter rule left Format unreachable *and* advertising a shortcut
      // that belonged to File. Take the first character not already claimed —
      // which is how Windows menus have always assigned these.
      let at = [...name].findIndex((ch) => !mnemonics.has(ch.toLowerCase()));
      if (at < 0) at = 0; // every letter taken: no mnemonic, but still labelled
      const key = name[at].toLowerCase();
      if (!mnemonics.has(key)) mnemonics.set(key, i);
      // The letter is wrapped so it can be underlined without changing layout.
      btn.innerHTML =
        `${name.slice(0, at)}<span class="mn">${name[at]}</span>${name.slice(at + 1)}`;
      btn.setAttribute("aria-keyshortcuts", `Alt+${name[at].toUpperCase()}`);
      btn.setAttribute("role", "menuitem");
      // Roving tabindex: the bar is one tab stop, not nine — Tab moves past it,
      // arrows move within it, which is what a menubar is supposed to do.
      btn.tabIndex = i === 0 ? 0 : -1;
      btn.setAttribute("aria-haspopup", "true"); btn.setAttribute("aria-expanded", "false");
      const drop = document.createElement("div"); drop.className = "menu-drop popmenu"; drop.hidden = true;
      drop.setAttribute("role", "menu");
      document.body.appendChild(drop); renderItems(drop, items, true);
      btn.addEventListener("click", (e) => { e.stopPropagation(); openIdx === i ? closeMenus() : openMenu(i); });
      btn.addEventListener("mouseenter", () => { if (openIdx >= 0 && openIdx !== i) openMenu(i); });
      bar.appendChild(btn); topBtns.push(btn); drops.push(drop);
    });

    // Alt+letter opens the matching menu; holding Alt alone reveals which letter
    // each menu answers to.
    document.addEventListener("keydown", (e) => {
      if (e.key === "Alt") { bar.classList.add("show-mnemonics"); return; }
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      const i = mnemonics.get((e.key || "").toLowerCase());
      if (i === undefined) return;
      e.preventDefault();
      openMenu(i);
      const first = drops[i].querySelector("button");
      if (first) first.focus();
    });
    const clearMnemonics = () => bar.classList.remove("show-mnemonics");
    document.addEventListener("keyup", (e) => { if (e.key === "Alt") clearMnemonics(); });
    // Alt+Tab and the like leave the key "held" as far as this document is
    // concerned, so drop the hint whenever the window loses focus too.
    window.addEventListener("blur", clearMnemonics);

    // Keyboard navigation for the menu bar and the open menu.
    const focusTop = (i) => {
      const n = topBtns.length;
      const at = ((i % n) + n) % n;
      for (const [j, b] of topBtns.entries()) b.tabIndex = j === at ? 0 : -1;
      topBtns[at].focus();
      return at;
    };
    // The items of the open drop, in visual order, skipping separators.
    const dropItems = () => (openIdx < 0 ? [] : [...drops[openIdx].querySelectorAll("button")]);
    const focusItem = (list, i) => {
      if (!list.length) return;
      const n = list.length;
      list[((i % n) + n) % n].focus();
    };
    bar.addEventListener("keydown", (e) => {
      const i = topBtns.indexOf(document.activeElement);
      if (i < 0) return;
      if (e.key === "ArrowRight") { const at = focusTop(i + 1); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "ArrowLeft") { const at = focusTop(i - 1); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "Home") { const at = focusTop(0); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "End") { const at = focusTop(topBtns.length - 1); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        openMenu(i);
        focusItem(dropItems(), 0);
        e.preventDefault();
      } else if (e.key === "Escape") { closeMenus(); e.preventDefault(); }
    });
    // Within an open menu: up/down through the items, left/right to the
    // neighbouring menu, Escape back to the bar.
    document.addEventListener("keydown", (e) => {
      if (openIdx < 0) return;
      const list = dropItems();
      const at = list.indexOf(document.activeElement);
      if (at < 0) return;
      if (e.key === "ArrowDown") { focusItem(list, at + 1); e.preventDefault(); }
      else if (e.key === "ArrowUp") { focusItem(list, at - 1); e.preventDefault(); }
      else if (e.key === "Home") { focusItem(list, 0); e.preventDefault(); }
      else if (e.key === "End") { focusItem(list, list.length - 1); e.preventDefault(); }
      else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        const next = focusTop(openIdx + (e.key === "ArrowRight" ? 1 : -1));
        openMenu(next);
        focusItem(dropItems(), 0);
        e.preventDefault();
      } else if (e.key === "Escape") { const i = openIdx; closeMenus(); focusTop(i); e.preventDefault(); }
    });

    document.addEventListener("click", (e) => {
      if (openIdx < 0) return;
      const inMenu = e.target.closest(".menubar, .menu-drop, .menu-sub");
      if (!inMenu) closeMenus();
    });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeMenus(); });

    // Help modal close wiring.
    const modal = document.getElementById("oc-modal");
    document.getElementById("oc-modal-x").addEventListener("click", () => { modal.hidden = true; });
    modal.addEventListener("click", (e) => { if (e.target === modal) modal.hidden = true; });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") modal.hidden = true; });
  }
  // Esc closes the tool panel (when no context menu is open and not editing).
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && activePanel && !state.editing && !document.getElementById("sheet-ctx")) {
      closePanel();
    }
  });
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
  const mod = await import(`./pkg/casual_calc_wasm.js?b=${BUILD}`);
  init = mod.default;
  wasm = mod;
  await init(`./pkg/casual_calc_wasm_bg.wasm?b=${BUILD}`);
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
