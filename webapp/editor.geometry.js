// Coordinates and hit-testing — cell to pixel and back, and what sits
// under a point.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  AUTOSCROLL_EDGE,
  COL_W,
  FREEZE_GRAB,
  FREEZE_HANDLE,
  HH,
  HW,
  MIN_LINE,
  RESIZE_GRAB,
  ROW_H,
  canvas,
  colWAt,
  colXAt,
  ctx,
  dragPos,
  draw,
  emuToPx,
  extend,
  filterButtons,
  firstBodyCol,
  firstBodyRow,
  firstDrawnCol,
  firstDrawnRow,
  fscreenXEnd,
  fscreenYEnd,
  geo,
  growthBefore,
  growthDirty,
  growthPrefix,
  growthRows,
  growthTotal,
  hiddenColMarks,
  hiddenRowMarks,
  invalidateGrowth,
  navBlock,
  off,
  on,
  outlineToggles,
  rebuildGrowth,
  rowHAt,
  rowYAt,
  scrollMeta,
  sheetMerges,
  start,
  state,
  status,
  t,
  tryEdit,
  wasm,
  wrap,
} from "./editor.core.js";

export function anchoredRect(o) {
  const x0 = fscreenX(o.c0) + emuToPx(o.fx || 0);
  const y0 = fscreenY(o.r0) + emuToPx(o.fy || 0);
  const x1 = fscreenXEnd(o.c1) + emuToPx(o.tx || 0);
  const y1 = fscreenYEnd(o.r1) + emuToPx(o.ty || 0);
  return { x: x0, y: y0, w: Math.max(8, x1 - x0), h: Math.max(8, y1 - y0) };
}

export function valueExtent(series) {
  let lo = 0, hi = 0;
  for (const s of series) {
    for (const v of s.values) {
      if (v === null) continue;
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
  }
  if (lo === hi) hi = lo + 1;
  return { lo, hi };
}

export function selRect() {
  // While Enter/Tab are walking a block, the block is the selection — deriving
  // it from anchor+sel would shrink it as the active cell moves inside.
  if (navBlock) return { ...navBlock };
  return {
    r0: Math.min(state.anchor.row, state.sel.row),
    c0: Math.min(state.anchor.col, state.sel.col),
    r1: Math.max(state.anchor.row, state.sel.row),
    c1: Math.max(state.anchor.col, state.sel.col),
  };
}

export function colWidthOf(col) {
  const drawn = geo.colOf.has(col) ? geo.colW[geo.colOf.get(col)] : undefined;
  if (drawn !== undefined && drawn > 0) return drawn;
  try { return JSON.parse(wasm.session_col_px(state.sheet, col, 1))[0] ?? COL_W; }
  catch { return COL_W; }
}

export function rowOffsetPx(row) {
  return wasm.session_row_offset_px(state.sheet, row) + growthBefore(row);
}

export function rowAtPx(px) {
  if (growthDirty) rebuildGrowth();
  const want = Math.max(0, Math.round(px));
  // With nothing grown there is nothing to correct for, and every extra engine
  // call rebuilds the sheet's geometry from scratch. This stays one call a
  // frame on the ordinary sheet.
  if (!growthTotal) return wasm.session_row_at_px(state.sheet, want);

  // **A search over the growth segments, not a fixed-point iteration.**
  //
  // This used to iterate `session_row_at_px(px - growthBefore(guess))` four
  // times, on the stated reasoning that "growth is monotonic, so it converges
  // in a couple of steps". It does not converge: for a guess *above* the answer
  // it subtracts the growth of every grown row above it, which drives the
  // argument negative, the engine clamps that to row 0, and the next step lands
  // back on the original guess. A 2-cycle, and it returned whichever end of it
  // the loop happened to stop on.
  //
  // Measured on a reported file: `rowAtPx(200)` answered row 10, whose top edge
  // is at 896px. At a scroll of 800 it answered a row past the last one, so the
  // frame contained no rows at all and the grid painted blank — reported as
  // "upper rows become empty and flicker", the flicker being that every scroll
  // step landed somewhere different.
  //
  // `growthBefore` is a step function, constant between consecutive grown rows,
  // so within one segment the inverse *is* exactly the engine's own — one call,
  // with a fixed offset subtracted. Which segment is a monotone predicate, so
  // it is a binary search over `growthRows` and costs log2(grown rows) engine
  // calls rather than an unbounded guess.
  const n = growthRows.length;
  let lo = 0, hi = n;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    // Rows in this segment carry `growthPrefix[mid]` of growth above them; the
    // segment ends at `growthRows[mid]`, the mid'th grown row itself.
    const top = wasm.session_row_offset_px(state.sheet, growthRows[mid]) + growthPrefix[mid];
    if (top <= want) lo = mid + 1; else hi = mid;
  }
  // `lo` is the count of grown rows whose rendered top edge is at or *below*
  // `want` — the loop advances while `top <= want`. Every row at the answer
  // therefore has exactly those `lo` grown rows above it, so it carries
  // `growthPrefix[lo]` of growth and the engine can be asked the rest directly.
  const answer = wasm.session_row_at_px(
    state.sheet,
    Math.max(0, want - growthPrefix[lo]),
  );
  // The engine's answer can only be one segment out at the boundary, where a
  // grown row's own band spans the pixel. Clamp into the segment rather than
  // trusting it: the segment bound is exact and the engine does not know about
  // growth at all.
  const floor = lo > 0 ? growthRows[lo - 1] : 0;
  return Math.max(floor, answer);
}

export function screenX(col) { return wasm.session_col_offset_px(state.sheet, col) - state.scrollX + HW; }

export function screenY(row) { return rowOffsetPx(row) - state.scrollY + HH; }

export function fscreenX(col) {
  const x = colXAt(col);
  if (x !== undefined) return x;
  const f = state.freeze || { fc: 0 };
  const o = wasm.session_col_offset_px(state.sheet, col);
  return HW + (col < f.fc ? o : o - state.scrollX);
}

export function fscreenY(row) {
  const y = rowYAt(row);
  if (y !== undefined) return y;
  const f = state.freeze || { fr: 0 };
  const o = rowOffsetPx(row);
  return HH + (row < f.fr ? o : o - state.scrollY);
}

export function mergeAt(row, col) {
  return sheetMerges.find((m) => row >= m.r0 && row <= m.r1 && col >= m.c0 && col <= m.c1);
}

export function cellPx(it) { return Math.round(((it.fs || 11) * 4) / 3); }

export function clampScroll() {
  state.scrollY = Math.max(0, Math.min(state.scrollY, scrollMeta.maxScrollY));
  state.scrollX = Math.max(0, Math.min(state.scrollX, scrollMeta.maxScrollX));
}

export function boundaryAt(px, py) {
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

export function spanX(c0, c1, v) {
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

export function spanY(r0, r1, v) {
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

export function resize() {
  const rect = wrap.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(rect.width * dpr);
  canvas.height = Math.floor(rect.height * dpr);
  canvas.style.width = rect.width + "px";
  canvas.style.height = rect.height + "px";
  ctx.setTransform(dpr * state.zoom, 0, 0, dpr * state.zoom, 0, 0);
  draw();
}

export function freezeHandleAt(px, py) {
  if (px >= HW || py >= HH) return null;
  const F = state.freeze;
  if (F.fc === 0 && px >= HW - FREEZE_HANDLE && py <= HH * 0.62) return { axis: "col" };
  if (F.fr === 0 && py >= HH - FREEZE_HANDLE && px <= HW * 0.62) return { axis: "row" };
  return null;
}

export function freezeHit(px, py) {
  const F = state.freeze;
  if (F.fc > 0 && py > HH && Math.abs(px - F.bodyX0) <= FREEZE_GRAB) return { axis: "col" };
  if (F.fr > 0 && px > HW && Math.abs(py - F.bodyY0) <= FREEZE_GRAB) return { axis: "row" };
  return null;
}

export function cellAt(px, py) {
  if (px < HW || py < HH) return null;
  return { row: rowAtY(py), col: colAtX(px) };
}

export function snapLeading(px, limit, frozen, isCol) {
  if (px >= limit) return limit;
  const abs = Math.max(0, px + frozen);
  const at = isCol ? wasm.session_col_at_px(state.sheet, Math.round(abs)) : rowAtPx(abs);
  const startOf = (i) =>
    (isCol ? wasm.session_col_offset_px(state.sheet, i) : rowOffsetPx(i)) - frozen;
  const start = startOf(at);
  return Math.min(start >= px ? start : startOf(at + 1), limit);
}

export function ensureVisible(row = state.sel.row, col = state.sel.col) {
  if (!wasm) return;
  const rect = wrap.getBoundingClientRect();
  const f = state.freeze || { fc: 0, fr: 0, bodyX0: HW, bodyY0: HH };
  // The scrolling viewport is what remains right of / below the frozen bands —
  // **in grid units**, which is why the rect is divided by the zoom first.
  //
  // `getBoundingClientRect` is CSS pixels; `bodyX0`, the column offsets and
  // `state.scrollX` are all grid units, and the canvas is what applies the
  // magnification between them. Subtracting one from the other without
  // converting made the viewport look `zoom` times larger than it is, so at
  // 200% every scroll-into-view believed it had twice the room and overshot —
  // the cell arrived on screen, which is why this went unnoticed, but at the
  // wrong end of a jump that moved further than it needed to. Every pointer
  // path in this file already divides; this one did not.
  const z = state.zoom || 1;
  const viewW = rect.width / z - f.bodyX0;
  const viewH = rect.height / z - f.bodyY0;
  const frozenW = fzOffset(col, true, f.fc);
  const frozenH = fzOffset(row, false, f.fr);
  // Frozen cells are always visible; only scroll for cells in the body region.
  if (col >= f.fc) {
    const cL = wasm.session_col_offset_px(state.sheet, col) - frozenW;
    const cW = JSON.parse(wasm.session_col_px(state.sheet, col, 1))[0] || COL_W;
    if (cL < state.scrollX) state.scrollX = cL;
    else if (cL + cW > state.scrollX + viewW) state.scrollX = snapLeading(cL + cW - viewW, cL, frozenW, true);
  }
  if (row >= f.fr) {
    const rT = rowOffsetPx(row) - frozenH;
    const rH = JSON.parse(wasm.session_row_px(state.sheet, row, 1))[0] || ROW_H;
    if (rT < state.scrollY) state.scrollY = rT;
    else if (rT + rH > state.scrollY + viewH) state.scrollY = snapLeading(rT + rH - viewH, rT, frozenH, false);
  }
  state.scrollX = Math.max(0, state.scrollX);
  state.scrollY = Math.max(0, state.scrollY);
}

export function edgeVelocity() {
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

export function fzOffset(_line, columns, count) {
  if (count <= 0) return 0;
  return columns
    ? wasm.session_col_offset_px(state.sheet, count)
    : rowOffsetPx(count);
}

export function usedBounds() {
  const b = JSON.parse(wasm.session_used_bounds(state.sheet));
  return { rows: Math.max(1, b.rows), cols: Math.max(1, b.cols) };
}

export function hiddenMarkAt(px, py) {
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

export function unhideMark(hit) {
  const { from, to } = hit.mark;
  const n = to - from + 1;
  tryEdit(() =>
    hit.axis === "col"
      ? wasm.session_unhide_cols(state.sheet, from, to)
      : wasm.session_unhide_rows(state.sheet, from, to),
  );
  status.textContent = `showed ${n} hidden ${hit.axis === "col" ? "column" : "row"}${n === 1 ? "" : "s"}`;
}

export function outlineToggleAt(px, py) {
  return outlineToggles.find(
    (t) => px >= t.x && px <= t.x + t.w && py >= t.y && py <= t.y + t.h,
  );
}

export function colAtX(px) {
  for (let i = 0; i < geo.colX.length; i++) if (px < geo.colX[i] + geo.colW[i]) return geo.colIdx[i];
  return geo.colIdx[geo.colIdx.length - 1] ?? state.firstCol;
}

export function rowAtY(py) {
  for (let i = 0; i < geo.rowY.length; i++) if (py < geo.rowY[i] + geo.rowH[i]) return geo.rowIdx[i];
  return geo.rowIdx[geo.rowIdx.length - 1] ?? state.firstRow;
}

export function effectiveRange() {
  const r = selRect();
  const b = usedBounds();
  if (state.selKind === "all") return { r0: 0, c0: 0, r1: b.rows - 1, c1: b.cols - 1 };
  if (state.selKind === "rows") return { r0: r.r0, c0: 0, r1: r.r1, c1: b.cols - 1 };
  if (state.selKind === "cols") return { r0: 0, c0: r.c0, r1: b.rows - 1, c1: r.c1 };
  return r;
}

export function filterButtonAt(px, py) {
  return filterButtons.find(
    (b) => px >= b.x && px <= b.x + b.w && py >= b.y && py <= b.y + b.h,
  );
}

export function anchorPoint(px, py) {
  // Clamped once and used for both halves: taking the cell from a clamped
  // coordinate and the offset from an unclamped one puts the two out of step,
  // and an edge dragged past the frozen headers lands in the wrong cell by the
  // amount it overshot.
  const x = Math.max(HW, px), y = Math.max(HH, py);
  const row = rowAtY(y), col = colAtX(x);
  return { row, col, dx: x - fscreenX(col), dy: y - fscreenY(row) };
}

export function updateGridCounts() {
  if (!wasm) return;
  try {
    const b = usedBounds();
    const lastRow = geo.rowIdx?.length ? geo.rowIdx[geo.rowIdx.length - 1] + 1 : 0;
    const lastCol = geo.colIdx?.length ? geo.colIdx[geo.colIdx.length - 1] + 1 : 0;
    canvas.setAttribute("aria-rowcount", String(Math.max(b.rows + 30, lastRow) + 1));
    canvas.setAttribute("aria-colcount", String(Math.max(b.cols + 8, lastCol) + 1));
  } catch {}
}

export function sheetNameAt(i) {
  try { return JSON.parse(wasm.session_sheet_names())[i]; } catch { return null; }
}

export function updateResize(px, py) {
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
  // Tell the *engine* the provisional size too, not just our own geometry.
  // Overriding `geo` alone moved the column edge while the text stayed where it
  // was, so content only re-wrapped on release. The engine supplies the display
  // text, so it has to know the width for the text to reflow with the drag.
  //
  // Unrecorded on purpose — a drag is not an edit until it is let go. Only the
  // single-line case: `all` and `band` scopes would mean writing every line on
  // every mouse move, which is a different cost question.
  if (state.resize.scope === "one") {
    try {
      wasm.session_preview_line_size(
        state.sheet,
        state.resize.index,
        state.resize.previewPx,
        state.resize.axis === "col",
      );
      invalidateGrowth();
    } catch { /* a protected sheet refuses; the drag simply shows nothing */ }
  }
  draw();
}

export function relayout() {
  invalidateGrowth();
  resize();
}
