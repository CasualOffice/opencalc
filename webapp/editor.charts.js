// Chart selection and the chart panel.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  activePanel,
  buildChartPanel,
  byId,
  chartDrag,
  chartFrames,
  draw,
  errText,
  invalidateGrowth,
  panelChart,
  state,
  status,
  statusError,
  wasm,
} from "./editor.core.js";

export function chartHandlePoints(f) {
  const mx = f.x + f.w / 2, my = f.y + f.h / 2;
  return [
    [f.x, f.y], [f.x + f.w, f.y], [f.x, f.y + f.h], [f.x + f.w, f.y + f.h],
    [mx, f.y], [mx, f.y + f.h], [f.x, my], [f.x + f.w, my],
  ];
}

export function chartDragRect() {
  const f = chartFrames.find((x) => x.index === chartDrag.index);
  if (!f) return null;
  const dx = chartDrag.px - chartDrag.x0, dy = chartDrag.py - chartDrag.y0;
  if (chartDrag.handle === null) return { x: f.x + dx, y: f.y + dy, w: f.w, h: f.h };
  // Which edges the grabbed handle moves. A mid-edge handle moves one.
  const [hx, hy] = chartHandlePoints(f)[chartDrag.handle];
  const left = Math.abs(hx - f.x) < 1, right = Math.abs(hx - (f.x + f.w)) < 1;
  const top = Math.abs(hy - f.y) < 1, bottom = Math.abs(hy - (f.y + f.h)) < 1;
  let { x, y, w, h } = f;
  if (left) { x += dx; w -= dx; }
  if (right) { w += dx; }
  if (top) { y += dy; h -= dy; }
  if (bottom) { h += dy; }
  return { x, y, w: Math.max(24, w), h: Math.max(24, h) };
}

export function chartAt(row, col) {
  try { return JSON.parse(wasm.session_chart_at(state.sheet, row, col)); } catch { return null; }
}

export function currentChart() {
  if (!panelChart) return null;
  try {
    return JSON.parse(wasm.session_chart_defs(panelChart.sheet))[panelChart.index] || null;
  } catch { return null; }
}

export function applyChart(c) {
  try {
    const dropped = wasm.session_set_chart(panelChart.sheet, panelChart.index, JSON.stringify(c));
    status.textContent = dropped
      ? "chart updated — Excel's own chart definition was replaced"
      : "chart updated";
  } catch (e) { statusError(errText(e)); }
  invalidateGrowth();
  draw();
  refreshChartPanel();
}

export function refreshChartPanel() {
  if (activePanel !== "chart") return;
  const body = byId("side-panel-body");
  body.textContent = "";
  buildChartPanel(body);
}
