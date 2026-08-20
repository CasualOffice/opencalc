// OpenCalc canvas grid editor. The WASM engine owns the workbook, computes the
// layout + display text, and recalculates; this file draws the grid and text on
// a canvas and routes edits back to the engine.
// The glue + wasm binary are loaded in main() with a build tag on the URL so a
// rebuilt engine is never shadowed by a stale browser cache.
//
// The tag is this file's own cache-buster, which the dev server stamps from the
// file's mtime. It was a hand-pinned constant, which meant a rebuilt engine
// kept the same module URL and Chrome went on serving the previous glue from
// its module map — `no-store` does not evict an ES module that is already
// resolved. The same mistake cost an afternoon of testing a fix that had never
// reached the browser; deriving the tag rather than remembering to bump it is
// what stops it recurring.
const BUILD = new URL(import.meta.url).searchParams.get("v") || "dev";
let init, wasm;

// --- White-labelling --------------------------------------------------------
//
// An integrator reselling a spreadsheet editor cannot ship one with somebody
// else's name in the toolbar, which is why every product in this market has it.
// Both of these come off the page's own URL so an embedding host sets them
// without a build: the WOPI adapter appends them when it points its iframe here
// (docs/74), and an SDK embedder can do the same.
//
// Bounded and escaped at the edge. `BRAND` reaches innerHTML in the About
// dialog, and `ACCENT` reaches a CSS custom property — a colour that is allowed
// to be arbitrary text can close the declaration and start another.
const PARAMS = new URL(location.href).searchParams;
const BRAND = (PARAMS.get("brand") || "OpenCalc").slice(0, 60);
const ACCENT = /^#[0-9a-f]{3,8}$|^[a-z]{3,20}$/i.test(PARAMS.get("accent") || "")
  ? PARAMS.get("accent")
  : null;

/// Chrome regions to hide, from `?hide=header,statusbar`.
///
/// The regions and the `oc-hide-*` classes are **the same ones `embed.js`
/// uses** — not a second vocabulary. `<opencalc-sheet>` already hides the
/// header by default, on the stated grounds that "an embedded editor is the
/// host's product, not ours". A host that iframes `editor.html` directly had
/// no way to say the same thing, so `casual-calc-host` rendered its own header
/// *and* got this one: two headers stacked, which is what a user running the
/// Docker stack sees. The comment in that page even claims it "can keep its
/// own chrome without forking the editor's HTML" — the intent was right and
/// nothing implemented it.
///
/// Filtered against the known list rather than trusted: this arrives on a URL,
/// so anybody who can hand somebody a link chooses it, and an unfiltered value
/// would be an arbitrary class on the document root.
const CHROME_REGIONS = [
  "header", "menubar", "toolbar", "formulabar", "tabs", "statusbar", "localepicker",
];
const HIDDEN_CHROME = (PARAMS.get("hide") || "")
  .split(",")
  .map((name) => name.trim().toLowerCase())
  .filter((name) => CHROME_REGIONS.includes(name));

/// Text that is about to become markup.
function htmlText(raw) {
  return String(raw).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

if (BRAND !== "OpenCalc") {
  document.title = BRAND;
  // `textContent`, not innerHTML: this is a name, and names contain `&`.
  for (const node of document.querySelectorAll(".tb-brand")) node.textContent = BRAND;
}
if (ACCENT) document.documentElement.style.setProperty("--oc-accent-color", ACCENT);

// Applied to the root, because `.oc-hide-header` and its siblings are written
// as descendant selectors and the header is a child of `<body>` here where in
// the embed element it is a child of the shell.
for (const region of HIDDEN_CHROME) {
  document.documentElement.classList.add(`oc-hide-${region}`);
}

// --- Mount root -------------------------------------------------------------
//
// Every DOM lookup goes through here rather than through `document`, so the
// same editor runs as a page *and* inside a shadow root. That is what lets a
// host embed it without its own stylesheet reaching in: a shadow boundary is
// the only thing in the platform that actually stops CSS, and it also means the
// editor's own selectors cannot leak out onto the host's page.
//
// `document` is still right for three things and they are left alone:
// `createElement` (nodes are not scoped), listeners that must catch events
// anywhere on the page (a mouse-up after the pointer leaves the grid), and the
// clipboard. Anything that *finds* an element in our markup, or parents a
// floating layer over it, uses these.
let ocRoot = document;
/// The node floating layers (menus, tooltips, submenus) attach to.
///
/// Inside a shadow root that is the root itself: appending to `document.body`
/// would put the menu outside the boundary, where our stylesheet does not reach
/// and the host's does — so it would come out unstyled and inherit theirs.
let ocOverlayHost = document.body;
/// The element carrying `data-theme` and the accent override.
///
/// The page's `<html>` when running as a page; the host element when embedded,
/// so a theme switch inside one embedded editor does not restyle the page
/// around it — or a second editor beside it.
let ocThemeHost = document.documentElement;

/// Point the editor at a mount root. Called by the embed wrapper before `main`.
export function setMountRoot(root) {
  ocRoot = root;
  ocOverlayHost = root === document ? document.body : root;
  ocThemeHost = root === document ? document.documentElement : root.host;
}

const byId = (id) => ocRoot.getElementById(id);
const qs = (sel) => ocRoot.querySelector(sel);
const qsa = (sel) => ocRoot.querySelectorAll(sel);
/// The focused element *within this mount*. A shadow root reports its own.
const activeEl = () => ocRoot.activeElement;


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
  endMode: false, // Excel's End mode: armed by `End`, spent by the next arrow
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
// "r,c" of linked cells in view. Kept as a set rather than asked per cell so a
// screenful of links costs one call, and so the click handler can tell in O(1)
// whether a click is on a link before doing anything slower.
let linkCells = new Set();
// Tables on the current sheet, refreshed each draw. Held so the header
// filter-button hit test does not have to ask the engine on every mousedown.
let tablesInView = [];
// Decoded pictures, keyed by package part.
//
// An <img> decodes asynchronously, so the first frame after a load has nothing
// to draw and a redraw has to be asked for once it does — otherwise the picture
// only appears the next time something else happens to repaint.
const imageCache = new Map();
function imageFor(part) {
  const hit = imageCache.get(part);
  if (hit !== undefined) return hit;
  imageCache.set(part, null); // in flight; do not request it again every frame
  let url = "";
  try { url = wasm.session_image_data(part); } catch {}
  if (!url) return null;
  const img = new Image();
  img.onload = () => { imageCache.set(part, img); draw(); };
  // A part this browser cannot decode stays a blank frame rather than
  // retrying forever.
  img.onerror = () => imageCache.set(part, false);
  img.src = url;
  return null;
}

// The pixel rectangle an anchored object occupies.
//
// Three things this has to get right, each of which was wrong before:
//
//  * the far edge is the *trailing* edge of the last row and column, not the
//    leading one — `colXAt(c1)` drew every frame a column and a row short;
//  * `fscreen*` rather than the raw accessors, so an edge scrolled out of the
//    window still has a position. The old code bailed out when the top-left was
//    not drawn, which made a whole chart vanish the moment its first row
//    scrolled off the top;
//  * the EMU offsets, so an edge sits where it was dragged instead of snapping
//    to the nearest gridline.
function anchoredRect(o) {
  const x0 = fscreenX(o.c0) + emuToPx(o.fx || 0);
  const y0 = fscreenY(o.r0) + emuToPx(o.fy || 0);
  const x1 = fscreenXEnd(o.c1) + emuToPx(o.tx || 0);
  const y1 = fscreenYEnd(o.r1) + emuToPx(o.ty || 0);
  return { x: x0, y: y0, w: Math.max(8, x1 - x0), h: Math.max(8, y1 - y0) };
}
/// EMUs per CSS pixel at 96 dpi — OOXML's own constant.
const EMU_PER_PX = 9525;
const emuToPx = (emu) => emu / EMU_PER_PX;
const pxToEmu = (px) => Math.round(px * EMU_PER_PX);
/// Whether a rectangle is entirely outside the drawable area.
const offCanvas = (r) =>
  r.x + r.w < HW || r.y + r.h < HH || r.x > canvas.clientWidth || r.y > canvas.clientHeight;

// Pictures anchored on the sheet, drawn under the charts and over the cells.
function drawImages() {
  if (!wasm) return;
  let images = [];
  try { images = JSON.parse(wasm.session_images(state.sheet)); } catch { return; }
  for (const im of images) {
    const { x: x0, y: y0, w, h } = anchoredRect(im);
    if (offCanvas({ x: x0, y: y0, w, h })) continue;
    const img = imageFor(im.part);
    if (!img) continue;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x0, y0, w, h);
    ctx.clip();
    // Fitted inside the anchor and centred, keeping the aspect ratio: an
    // anchor is a frame, and stretching a photograph to fill it is a visible
    // lie about the file.
    const scale = Math.min(w / img.naturalWidth, h / img.naturalHeight);
    const dw = img.naturalWidth * scale, dh = img.naturalHeight * scale;
    ctx.drawImage(img, x0 + (w - dw) / 2, y0 + (h - dh) / 2, dw, dh);
    ctx.restore();
  }
}

// Chart frames from the last paint, for hit-testing; the selected chart; and
// the drag in progress. A chart floats over the grid rather than occupying
// cells, so none of this can come from the cell hit test.
let chartFrames = [];
let chartSel = null;
let chartDrag = null;
/// Handle radius, and the slop a click gets when aiming at one.
const CHART_HANDLE = 4;

// Every chart on the sheet, at its anchored cells.
//
// A chart is anchored in *cells*, which is why it moves with the rows under it
// and why this has to be recomputed each frame rather than positioned once.
function drawCharts(withQuad) {
  if (!wasm) return;
  chartFrames = [];
  let charts = [];
  try { charts = JSON.parse(wasm.session_charts(state.sheet)); } catch { return; }
  for (const [index, ch] of charts.entries()) {
    const { x: x0, y: y0, w, h } = anchoredRect(ch);
    // Kept for hit-testing: a chart floats over cells rather than occupying
    // them, so clicking one cannot go through the cell grid. Recorded even
    // when it is off-screen, so the frame list is a faithful account of where
    // every chart is rather than of which ones happen to be visible.
    chartFrames.push({ index, x: x0, y: y0, w, h });
    if (offCanvas({ x: x0, y: y0, w, h })) continue;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x0, y0, w, h);
    ctx.clip();
    drawChartFrame(ch, x0, y0, w, h);
    ctx.restore();
  }
  drawChartSelection();
}

// The selected chart's outline and its eight handles, plus the live outline
// while one is being dragged.
//
// Drawn after every chart so it is never painted over by a chart stacked on
// top of the selected one.
function drawChartSelection() {
  if (chartDrag) {
    const r = chartDragRect();
    if (r) {
      ctx.save();
      ctx.strokeStyle = colors.accent || "#4472C4";
      ctx.setLineDash([4, 3]);
      ctx.lineWidth = 1.5;
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w, r.h);
      ctx.restore();
    }
  }
  if (!chartSel || chartSel.sheet !== state.sheet) return;
  const frame = chartFrames.find((f) => f.index === chartSel.index);
  if (!frame) return;
  ctx.save();
  ctx.strokeStyle = colors.accent || "#4472C4";
  ctx.lineWidth = 1.5;
  ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);
  ctx.fillStyle = colors.bg;
  for (const [hx, hy] of chartHandlePoints(frame)) {
    ctx.beginPath();
    ctx.arc(hx, hy, CHART_HANDLE, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}

/// The eight resize handles, corners first so a corner wins where two overlap.
function chartHandlePoints(f) {
  const mx = f.x + f.w / 2, my = f.y + f.h / 2;
  return [
    [f.x, f.y], [f.x + f.w, f.y], [f.x, f.y + f.h], [f.x + f.w, f.y + f.h],
    [mx, f.y], [mx, f.y + f.h], [f.x, my], [f.x + f.w, my],
  ];
}

// The rectangle a drag is currently proposing, in pixels.
function chartDragRect() {
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

// One chart: frame, title, then whichever picture its kind calls for.
function drawChartFrame(ch, x, y, w, h) {
  ctx.fillStyle = colors.bg;
  ctx.fillRect(x, y, w, h);
  ctx.strokeStyle = colors.border || "#888";
  ctx.lineWidth = 1;
  ctx.strokeRect(x + 0.5, y + 0.5, w - 1, h - 1);

  let top = y + 6;
  if (ch.title) {
    ctx.fillStyle = colors.fg;
    ctx.font = "600 12px " + (colors.uiFont || "system-ui, sans-serif");
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.fillText(ch.title, x + w / 2, top);
    top += 18;
  }
  ctx.textAlign = "left";

  const series = (ch.series || []).filter((s) => (s.values || []).some((v) => v !== null));
  if (!series.length) {
    // Honest rather than blank: the chart exists, its data did not resolve.
    ctx.fillStyle = colors.muted || "#888";
    ctx.font = "11px " + (colors.uiFont || "system-ui, sans-serif");
    ctx.fillText("no data", x + 8, top);
    return;
  }
  const plot = { x: x + 34, y: top, w: w - 44, h: y + h - top - 18 };
  // The legend takes its side out of the plot before anything is drawn, or the
  // bars run underneath it. Two or more series without one are unreadable —
  // three grey rectangles and no way to tell which is which.
  const legend = ch.legend ? legendBox(ch, series, x, y, w, h, plot) : null;
  if (plot.w < 20 || plot.h < 20) return;
  if (legend) drawLegend(series, legend);
  // Axis titles sit outside the plot, so they come off it too.
  if (ch.xTitle) {
    ctx.save();
    ctx.fillStyle = colors.muted || "#888";
    ctx.font = "10px " + (colors.uiFont || "system-ui, sans-serif");
    ctx.textAlign = "center";
    ctx.fillText(ch.xTitle, plot.x + plot.w / 2, y + h - 11);
    ctx.restore();
    plot.h -= 11;
  }
  if (ch.yTitle) {
    ctx.save();
    ctx.fillStyle = colors.muted || "#888";
    ctx.font = "10px " + (colors.uiFont || "system-ui, sans-serif");
    ctx.textAlign = "center";
    ctx.translate(x + 10, plot.y + plot.h / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText(ch.yTitle, 0, 0);
    ctx.restore();
    plot.x += 10;
    plot.w -= 10;
  }
  if (plot.w < 20 || plot.h < 20) return;

  if (ch.kind === "pie" || ch.kind === "doughnut") drawPie(ch, series, plot);
  else if (ch.kind === "line" || ch.kind === "area" || ch.kind === "scatter") {
    drawLineChart(ch, series, plot, ch.kind);
  } else if (ch.kind === "bar" || ch.kind === "column") drawBarChart(ch, series, plot, ch.kind);
  else {
    ctx.fillStyle = colors.muted || "#888";
    ctx.font = "11px " + (colors.uiFont || "system-ui, sans-serif");
    ctx.fillText(`${ch.kind} chart not drawn`, plot.x, plot.y);
  }
}

/// Reserve the legend's side of the frame, shrinking `plot` to what is left.
///
/// Returns the rectangle the legend gets, or `null` when the frame is too small
/// to give it one — a legend that leaves no room for the plot has cost more
/// than it explains.
function legendBox(ch, series, x, y, w, h, plot) {
  ctx.font = "10px " + (colors.uiFont || "system-ui, sans-serif");
  const widest = Math.max(
    24,
    ...series.map((s, i) => ctx.measureText(s.name || `Series ${i + 1}`).width),
  );
  const side = ch.legend;
  if (side === "r" || side === "l" || side === "tr") {
    const width = Math.min(widest + 22, Math.floor(w * 0.4));
    if (plot.w - width < 40) return null;
    plot.w -= width;
    const left = side === "l" ? plot.x : plot.x + plot.w + 6;
    if (side === "l") plot.x += width;
    return { x: left, y: plot.y, w: width, h: plot.h, rows: true };
  }
  const height = 14;
  if (plot.h - height < 40) return null;
  plot.h -= height;
  const topEdge = side === "t" ? plot.y : plot.y + plot.h + 2;
  if (side === "t") plot.y += height;
  return { x: plot.x, y: topEdge, w: plot.w, h: height, rows: false };
}

/// Swatch and name per series, stacked down the side or run across the foot.
function drawLegend(series, box) {
  const palette = seriesColors(series.length);
  ctx.save();
  ctx.font = "10px " + (colors.uiFont || "system-ui, sans-serif");
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  let cursorX = box.x;
  series.forEach((s, i) => {
    const label = s.name || `Series ${i + 1}`;
    const cy = box.rows ? box.y + 8 + i * 14 : box.y + box.h / 2;
    const cx = box.rows ? box.x : cursorX;
    if (box.rows && cy > box.y + box.h) return;
    ctx.fillStyle = palette[i];
    ctx.fillRect(cx, cy - 4, 8, 8);
    ctx.fillStyle = colors.fg;
    ctx.fillText(label, cx + 12, cy);
    cursorX = cx + 12 + ctx.measureText(label).width + 10;
  });
  ctx.restore();
}

// Series colours, taken from the workbook's theme accents so a chart matches
// the file it came from rather than a palette invented here.
function seriesColors(n) {
  let theme = [];
  try { theme = JSON.parse(wasm.theme_colors()); } catch {}
  const accents = theme.slice(4, 10).filter(Boolean);
  const base = accents.length ? accents : ["4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47"];
  return Array.from({ length: n }, (_, i) => "#" + base[i % base.length]);
}

// The value range a chart's axis has to cover, always including zero so a bar's
// length is proportional to its value.
function valueExtent(series) {
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

function drawAxes(plot, lo, hi) {
  const zeroY = plot.y + plot.h * (hi / (hi - lo));
  ctx.strokeStyle = colors.gridline || "#666";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(plot.x, plot.y);
  ctx.lineTo(plot.x, plot.y + plot.h);
  ctx.moveTo(plot.x, zeroY + 0.5);
  ctx.lineTo(plot.x + plot.w, zeroY + 0.5);
  ctx.stroke();
  ctx.fillStyle = colors.muted || "#888";
  ctx.font = "9px " + (colors.uiFont || "system-ui, sans-serif");
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  ctx.fillText(String(Math.round(hi * 100) / 100), plot.x - 3, plot.y + 4);
  ctx.fillText(String(Math.round(lo * 100) / 100), plot.x - 3, plot.y + plot.h - 4);
  ctx.textAlign = "left";
  return zeroY;
}

function drawBarChart(ch, series, plot, kind) {
  const { lo, hi } = valueExtent(series);
  const zeroY = drawAxes(plot, lo, hi);
  const cols = seriesColors(series.length);
  const points = Math.max(...series.map((s) => s.values.length));
  if (!points) return;
  const groupW = plot.w / points;
  const barW = Math.max(1, (groupW * 0.7) / series.length);
  for (let i = 0; i < points; i++) {
    for (let si = 0; si < series.length; si++) {
      const v = series[si].values[i];
      if (v === null || v === undefined) continue;
      const bx = plot.x + i * groupW + groupW * 0.15 + si * barW;
      const top = plot.y + plot.h * ((hi - v) / (hi - lo));
      ctx.fillStyle = cols[si];
      // A negative value draws downward from the zero line, which is why the
      // rectangle is measured from it rather than from the axis.
      ctx.fillRect(bx, Math.min(top, zeroY), barW - 1, Math.abs(zeroY - top) || 1);
    }
  }
  drawCategoryLabels(ch, plot, points, kind);
}

function drawLineChart(ch, series, plot, kind) {
  const { lo, hi } = valueExtent(series);
  drawAxes(plot, lo, hi);
  const cols = seriesColors(series.length);
  const points = Math.max(...series.map((s) => s.values.length));
  const step = points > 1 ? plot.w / (points - 1) : 0;
  for (let si = 0; si < series.length; si++) {
    ctx.strokeStyle = cols[si];
    ctx.fillStyle = cols[si];
    ctx.lineWidth = 1.8;
    ctx.beginPath();
    let started = false;
    for (let i = 0; i < series[si].values.length; i++) {
      const v = series[si].values[i];
      if (v === null || v === undefined) { started = false; continue; }
      const px = plot.x + i * step;
      const py = plot.y + plot.h * ((hi - v) / (hi - lo));
      if (kind === "scatter") { ctx.fillRect(px - 2, py - 2, 4, 4); continue; }
      if (started) ctx.lineTo(px, py); else { ctx.moveTo(px, py); started = true; }
    }
    if (kind !== "scatter") ctx.stroke();
  }
  drawCategoryLabels(ch, plot, points, kind);
}

function drawPie(ch, series, plot) {
  const values = series[0].values.filter((v) => v !== null && v > 0);
  const total = values.reduce((a, b) => a + b, 0);
  if (!total) return;
  const cols = seriesColors(values.length);
  const cx = plot.x + plot.w / 2, cy = plot.y + plot.h / 2;
  const r = Math.max(6, Math.min(plot.w, plot.h) / 2 - 4);
  let angle = -Math.PI / 2; // twelve o'clock, as Excel starts
  values.forEach((v, i) => {
    const sweep = (v / total) * Math.PI * 2;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.arc(cx, cy, r, angle, angle + sweep);
    ctx.closePath();
    ctx.fillStyle = cols[i];
    ctx.fill();
    angle += sweep;
  });
  if (ch.kind === "doughnut") {
    ctx.beginPath();
    ctx.arc(cx, cy, r * 0.55, 0, Math.PI * 2);
    ctx.fillStyle = colors.bg;
    ctx.fill();
  }
}

// Category labels under the plot, thinned to whatever fits: overlapping labels
// are less readable than fewer of them.
function drawCategoryLabels(ch, plot, points, kind) {
  const cats = ch.cats || [];
  if (!cats.length || kind === "scatter") return;
  ctx.fillStyle = colors.muted || "#888";
  ctx.font = "9px " + (colors.uiFont || "system-ui, sans-serif");
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const every = Math.max(1, Math.ceil((points * 34) / plot.w));
  for (let i = 0; i < points; i += every) {
    const label = cats[i];
    if (!label) continue;
    const px = points > 1 && kind !== "column" && kind !== "bar"
      ? plot.x + (i * plot.w) / (points - 1)
      : plot.x + (i + 0.5) * (plot.w / points);
    ctx.fillText(label.length > 10 ? label.slice(0, 9) + "…" : label, px, plot.y + plot.h + 3);
  }
  ctx.textAlign = "left";
}

// A cell's text colour. The cell's own wins; a table supplies one where the
// cell has none, because a table style's colours are part of the style, not of
// the cells — and because the block a table paints is light whatever the
// application theme is, so the grid's own text colour would vanish on it.
function cellFg(it) {
  if (it.fc) return "#" + it.fc;
  return tableTextAt(it.r, it.c) || colors.fg;
}
// The table text colour at a cell, or null where the cell's own style wins.
function tableTextAt(r, c) {
  for (const t of tablesInView) {
    if (r < t.r0 || r > t.r1 || c < t.c0 || c > t.c1) continue;
    const isHeader = t.headers > 0 && r === t.r0;
    const isTotals = t.totals > 0 && r === t.r1;
    // The header's colour is part of the style — white on a Medium accent —
    // and the body's is whatever reads against the light block a table is.
    return isHeader || isTotals ? "#" + t.headerText : "#" + t.bodyText;
  }
  return null;
}
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
  "#CALC!": "The calculation produced nothing — a filter that matched no rows, usually.",
};
let dragPos = null; // latest pointer {px,py} during a selection/fill drag
let autoRaf = 0; // rAF handle for edge auto-scroll while dragging

// The normalized selection rectangle (inclusive) from anchor..focus.
function selRect() {
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
const DEFAULT_SCROLL_DAMP = 0.8; // rows-per-wheel factor; tunable in settings
let scrollDamp = DEFAULT_SCROLL_DAMP;

let canvas;
let ctx;
let wrap;
let inline;
let selStats;
let vscroll;
let vthumb;
let hscroll;
let hthumb;
let fInput;
let cellRef;
let commentTip;
let status;

/// A theme token's resolved value.
///
/// Read from the mount's own root rather than `document.body`, or an embedded
/// editor would paint its canvas from the *host page's* tokens while its chrome
/// used ours.
const css = (name) =>
  getComputedStyle(ocRoot === document ? document.body : ocRoot.host)
    .getPropertyValue(name)
    .trim();
let colors = {};
function readColors() {
  colors = {
    bg: css("--oc-background-color") || "#fff",
    fg: css("--oc-text-color") || "#0b0d12",
    muted: css("--oc-muted-text-color") || "#7b8391",
    grid: css("--oc-gridline-color") || "#f0f1f4",
    headerBg: css("--oc-surface-color") || "#f6f7f9",
    accent: css("--oc-accent-color") || "#2f6df6",
    sel: css("--oc-selection-color") || "rgba(47,109,246,.10)",
    // Distinct from the selection tint: a find hit and the active cell must not
    // read as the same thing.
    findHit: css("--oc-find-highlight-color") || "rgba(245,158,11,.28)",
    // Table banding. Deliberately faint and theme-derived: a band is a reading
    // aid, and one strong enough to compete with a fill or a conditional
    // format would make the user's own formatting harder to see, not easier.
    tableHeader: css("--oc-table-header-color") || "rgba(47,109,246,.16)",
    tableBand: css("--oc-table-band-color") || "rgba(127,140,170,.09)",
    // Read from the theme rather than hardcoded: the freeze divider sits on the
    // grid, so it has to darken and lighten with it. `colors.freezeLine` was
    // already consulted at the draw site but never populated here, so the
    // fallback was always what showed.
    freezeLine: css("--oc-freeze-line-color") || "#5f6368",
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
// Measured heights, keyed by everything that can change one. Sheets repeat text
// heavily — a label column is the same string a thousand times — and measuring
// each occurrence separately was the bulk of a rebuild. Cleared on zoom, which
// is the only thing that changes the metrics without changing the key.
let heightMemo = new Map();

function neededRowHeight(it, colWidth) {
  const key = `${it.t}\u0000${it.w ? 1 : 0}\u0000${it.rot || 0}\u0000${it.fs || 0}\u0000${it.b ? 1 : 0}\u0000${it.i ? 1 : 0}\u0000${it.fn || ""}\u0000${Math.round(colWidth)}`;
  const hit = heightMemo.get(key);
  if (hit !== undefined) return hit;
  const value = measureRowHeight(it, colWidth);
  // Bounded so a sheet of entirely distinct strings cannot grow it without end.
  if (heightMemo.size < 50000) heightMemo.set(key, value);
  return value;
}

function measureRowHeight(it, colWidth) {
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
  if (it.w) {
    // Explicit newlines split first, then each segment wraps: a hard break is a
    // break whether or not the text after it would have fitted.
    const lines = String(it.t)
      .split("\n")
      .flatMap((seg) => wrapLines({ ...it, t: seg }, colWidth - 8));
    return lines.length * cellLineH(it) + 6;
  }
  // Newlines make a cell tall even without wrap, which this did not account
  // for: `autofitRow` had its own copy of this arithmetic that did, so the two
  // disagreed about the same cell — and the copy was the one missing rotation.
  // Both cases live here now.
  const hard = String(it.t).split("\n").length;
  if (hard > 1) return hard * cellLineH(it) + 6;
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

// Zoom changes the metrics without changing any memo key, so the memo has to go.
function clearHeightMemo() { heightMemo = new Map(); }

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
  const row = wasm.session_row_at_px(state.sheet, Math.round(px));
  // With nothing grown there is nothing to correct for, and every extra engine
  // call rebuilds the sheet's geometry from scratch. This loop ran regardless,
  // turning one call a frame into five on every sheet.
  if (!growthTotal) return row;
  let at = row;
  for (let i = 0; i < 4; i++) {
    const next = wasm.session_row_at_px(state.sheet, Math.round(px - growthBefore(at)));
    if (next === at) break;
    at = next;
  }
  return at;
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

// The font for one rich-text run: the run's own properties where it states
// them, the cell's where it does not. A run inherits rather than replaces —
// `<rPr>` carries only what differs, so treating an absent property as a reset
// would drop the cell's font on every partially-formatted string.
function runFont(it, run) {
  const merged = {
    b: run.b ?? it.b,
    i: run.i ?? it.i,
    fn: run.fn ?? it.fn,
    // Superscript and subscript are drawn smaller, as every renderer does;
    // the format states the position, not the size.
    fs: run.fs ?? it.fs,
  };
  const px = run.va ? Math.max(7, cellPx(merged) * 0.72) : cellPx(merged);
  const weight = merged.b ? "600 " : "";
  const slant = merged.i ? "italic " : "";
  return `${slant}${weight}${px}px ${fontStack(merged.fn)}`;
}

// Total width of a rich string, measured run by run — each run has its own
// font, so measuring the concatenated text with one font gives a width that is
// wrong wherever the runs differ, and the alignment then drifts.
function runsWidth(it) {
  const saved = ctx.font;
  let total = 0;
  for (const run of it.runs) {
    ctx.font = runFont(it, run);
    total += ctx.measureText(run.t).width;
  }
  ctx.font = saved;
  return total;
}

// Draw a rich string starting at `x` on baseline `y`, returning the width used.
function drawRuns(it, x, y) {
  const saved = ctx.font;
  const savedFill = ctx.fillStyle;
  let cursor = x;
  for (const run of it.runs) {
    ctx.font = runFont(it, run);
    ctx.fillStyle = run.fc ? "#" + run.fc : (cellFg(it));
    // Superscript rides above the baseline, subscript below it. The offsets
    // are fractions of the cell's size so they track a resized font.
    const shift = run.va === "superscript" ? -cellPx(it) * 0.32
      : run.va === "subscript" ? cellPx(it) * 0.18
      : 0;
    ctx.fillText(run.t, cursor, y + shift);
    const w = ctx.measureText(run.t).width;
    if (run.u || run.st) {
      const ly = run.st ? y + shift - cellPx(it) * 0.28 : y + shift + 2.5;
      ctx.fillRect(cursor, ly, w, 1);
    }
    cursor += w;
  }
  ctx.font = saved;
  ctx.fillStyle = savedFill;
  return cursor - x;
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
  const firstRowIdx = fr > 0 ? 0 : fsr;
  const firstColIdx = fc > 0 ? 0 : fsc;
  geoItems = JSON.parse(
    wasm.session_cells(state.sheet, firstRowIdx, firstColIdx, lastRowIdx, lastColIdx),
  );
  // Text spills across empty neighbours, so a label whose own cell is outside
  // the window can still be showing inside it — and only the *nearest*
  // populated cell on each side can, since anything beyond is blocked by it.
  // Without these a long label vanished the instant its own column scrolled
  // off, taking the visible half of the text with it. Excel keeps drawing it.
  //
  // Two calls at most, whatever the row count: everything between the furthest
  // owner and the window is empty by definition, so one window per side picks
  // up exactly the owners.
  try {
    const span = JSON.parse(
      wasm.session_spill_owners(state.sheet, firstRowIdx, lastRowIdx, firstColIdx, lastColIdx),
    );
    const gather = (a, b) => {
      for (const it of JSON.parse(
        wasm.session_cells(state.sheet, firstRowIdx, a, lastRowIdx, b),
      )) {
        // Only text can spill: a number too wide for its cell becomes `#`
        // inside it, and a wrapped or clipped cell stays put by definition.
        if (it.t && !it.n && !it.w && !it.cl && !it.shrink) geoItems.push(it);
      }
    };
    if (span.left !== null && firstColIdx > 0) gather(span.left, firstColIdx - 1);
    if (span.right !== null) gather(lastColIdx + 1, span.right);
  } catch { /* nothing outside the window is the common case */ }
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

// Hold the scroll inside the content extent. Wheeling or paging past the end
// used to leave the grid parked in blank space with no way back but scrolling
// the other way — the thumb had already bottomed out, so it gave no hint that
// anything had moved.
function clampScroll() {
  state.scrollY = Math.max(0, Math.min(state.scrollY, scrollMeta.maxScrollY));
  state.scrollX = Math.max(0, Math.min(state.scrollX, scrollMeta.maxScrollX));
}
function updateScrollbars(v) {
  if (!wasm) return;
  const b = usedBounds();
  const viewH = v.h - HH, viewW = v.w - HW;
  // A fixed buffer past the data, not one measured from the current scroll.
  // Including `scrollY` made the extent grow as you scrolled, so the end could
  // never be reached: scrolling past the data kept going forever, and every one
  // of those frames did the geometry work again. That is the drag past the last
  // row, and it is also why `clampScroll` had nothing to clamp to.
  const contentH = Math.max(rowOffsetPx(b.rows + 30), viewH + 1);
  const contentW = Math.max(
    wasm.session_col_offset_px(state.sheet, b.cols + 8),
    viewW + 1,
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
  const hdr = qs(".app-header");
  const btn = byId("hdr-collapse");
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
  clearHeightMemo();
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

  // Charts, drawn over the grid at their anchors. Read-only: the engine parses
  // the chart part for display and still writes it back from its own bytes, so
  // nothing here can change the file.
  drawImages();
  drawCharts(withQuad);

  // Manual page breaks, as the dashed rules Excel draws. A break that is only
  // visible once you print is a break you set by accident and never find.
  if (wasm) {
    let brk = { rows: [], cols: [] };
    try { brk = JSON.parse(wasm.session_page_breaks(state.sheet)); } catch {}
    if (brk.rows.length || brk.cols.length) {
      ctx.save();
      ctx.strokeStyle = colors.accent;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([6, 4]);
      for (const r of brk.rows) {
        const y = rowYAt(r);
        if (y === undefined) continue;
        ctx.beginPath();
        ctx.moveTo(HW, y + 0.5);
        ctx.lineTo(canvas.clientWidth, y + 0.5);
        ctx.stroke();
      }
      for (const c of brk.cols) {
        const x = colXAt(c);
        if (x === undefined) continue;
        ctx.beginPath();
        ctx.moveTo(x + 0.5, HH);
        ctx.lineTo(x + 0.5, canvas.clientHeight);
        ctx.stroke();
      }
      ctx.restore();
    }
  }

  // Tables: header shading and banded rows.
  //
  // This has to run before the cell pass, not after it: the fills are opaque,
  // and painting them later covers the text. The block used to sit near the end
  // of draw() with a comment claiming otherwise — it got away with it only
  // because the two colours it used were 16%-alpha washes.
  tablesInView = [];
  if (wasm) {
    try { tablesInView = JSON.parse(wasm.session_tables(state.sheet)); } catch {}
    for (const t of tablesInView) {
      // Colours come from the table's own style name resolved against the
      // workbook theme, not from constants here: a file whose author chose a
      // green style has to open green.
      //
      // The body is filled too, not just the bands. A table is a light block in
      // Excel whatever the application theme is, and painting only the bands
      // left light stripes across a dark grid with unreadable text on them.
      for (let r = t.r0; r <= t.r1; r++) {
        const ry = rowYAt(r);
        if (ry === undefined) continue;
        const rh = rowHAt(r);
        const isHeader = t.headers > 0 && r === t.r0;
        const isTotals = t.totals > 0 && r === t.r1;
        // Bands count from the first *data* row, so a header does not shift
        // the stripe pattern by one and make the first data row look banded
        // when it should not be.
        const dataIndex = r - t.r0 - (t.headers > 0 ? 1 : 0);
        const banded = t.stripes && !isHeader && !isTotals && dataIndex % 2 === 1;
        for (let c = t.c0; c <= t.c1; c++) {
          const cx = colXAt(c);
          if (cx === undefined) continue;
          const cw = colWAt(c);
          // A column stripe alternates on top of the row banding, and an
          // emphasised first/last column reads as banded too.
          const colBanded = t.colStripes && (c - t.c0) % 2 === 1;
          const emphasised = (t.firstCol && c === t.c0) || (t.lastCol && c === t.c1);
          const fill = (isHeader || isTotals)
            ? "#" + t.headerFill
            : (banded || colBanded || emphasised) ? "#" + t.bandFill : "#" + t.bodyFill;
          withQuad(r, c, () => { ctx.fillStyle = fill; ctx.fillRect(cx, ry, cw, rh); });
        }
        // The rule under the header is what a Light style has instead of a
        // fill, so it is drawn for every family rather than only where the
        // header is coloured.
        if (isHeader || isTotals) {
          const y = isHeader ? ry + rh - 1 : ry;
          for (let c = t.c0; c <= t.c1; c++) {
            const cx = colXAt(c);
            if (cx === undefined) continue;
            withQuad(r, c, () => {
              ctx.fillStyle = "#" + t.border;
              ctx.fillRect(cx, y, colWAt(c), 1);
            });
          }
        }
      }
    }
  }

  // Data bars sit behind the value: drawn after the cell fills so they read as
  // part of the cell, before the text so they never obscure it.
  ctx.textBaseline = "middle";
  // **The geometry comes from the engine, not from here.** These numbers used
  // to be written out twice — once in `casual-calc-render` and once in this
  // file — and agreed only because somebody had copied them across, which is
  // the two-renderers-can-disagree shape `conditional.rs` argues against,
  // reintroduced one layer up (`RND-08`). The canvas still paints; it no longer
  // decides.
  const bar = dataBarStyle();
  for (const it of items) {
    if (it.bar === undefined) continue;
    const bx = colXAt(it.c), by = rowYAt(it.r);
    if (bx === undefined || by === undefined) continue;
    const bw = Math.max(0, (colWAt(it.c) - 2 * bar.padX) * it.bar);
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#" + (it.barc || bar.defaultColor);
      ctx.globalAlpha = bar.alpha;
      ctx.fillRect(bx + bar.padX, by + bar.padY, bw, rowHAt(it.r) - 2 * bar.padY);
      ctx.globalAlpha = 1;
    });
  }
  for (const it of items) {
    if (!it.bg && !it.grad) continue;
    const x = colXAt(it.c);
    const y = rowYAt(it.r);
    if (x === undefined || y === undefined) continue;
    const w = colWAt(it.c) - 1, h = rowHAt(it.r) - 1;
    withQuad(it.r, it.c, () => {
      if (it.grad) {
        // A gradient replaces the fill rather than joining it: `<fill>` holds
        // a pattern or a gradient, never both. The angle is measured from the
        // horizontal, as OOXML states it.
        const rad = ((it.grad.deg || 0) * Math.PI) / 180;
        const g = ctx.createLinearGradient(
          x + 1, y + 1,
          x + 1 + Math.cos(rad) * w, y + 1 + Math.sin(rad) * h,
        );
        for (const stop of it.grad.stops) {
          g.addColorStop(Math.min(1, Math.max(0, stop.p)), "#" + stop.c);
        }
        ctx.fillStyle = g;
        ctx.fillRect(x + 1, y + 1, w, h);
        return;
      }
      if (it.pat) {
        // A pattern's *background* fills the cell and its foreground draws the
        // motif on top. Painting only the foreground — as a solid — is what
        // made every patterned cell look like a flat block of the wrong
        // colour. The motifs are approximated by density rather than matched
        // hatch for hatch: the alternative is eighteen bitmaps for something
        // almost no sheet uses, and a wrong-density hatch still reads as a
        // hatch.
        ctx.fillStyle = it.bg2 ? "#" + it.bg2 : colors.bg;
        ctx.fillRect(x + 1, y + 1, w, h);
        const density = { gray125: 0.125, gray0625: 0.0625, lightGray: 0.25,
          mediumGray: 0.5, darkGray: 0.75, lightGrid: 0.25, lightTrellis: 0.3,
          darkGrid: 0.6, darkTrellis: 0.65 }[it.pat] ?? 0.4;
        ctx.save();
        ctx.globalAlpha = density;
        ctx.fillStyle = "#" + it.bg;
        ctx.fillRect(x + 1, y + 1, w, h);
        ctx.restore();
        return;
      }
      ctx.fillStyle = "#" + it.bg;
      ctx.fillRect(x + 1, y + 1, w, h);
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
    // The row must be in the window, but the *column* need not: a spill owner
    // fetched from outside it is here precisely so its text can reach in, and
    // `colXAt` has no answer for a column that was never drawn.
    const yTop = rowYAt(it.r);
    if (yTop === undefined) continue;
    const drawnHere = geo.colOf.has(it.c);
    if (!drawnHere && (it.w || it.cl || it.shrink)) continue; // cannot spill
    const x = drawnHere ? colXAt(it.c) : fscreenX(it.c);
    const w = drawnHere ? colWAt(it.c) : fscreenXEnd(it.c) - x;
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
      ctx.fillStyle = cellFg(it);
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
      ctx.fillStyle = cellFg(it);
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
      ctx.fillStyle = cellFg(it);
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
      ctx.fillStyle = cellFg(it);
      ctx.textAlign = "center";
      ctx.fillText(String(it.t), (x + spanR) / 2, y);
      ctx.restore();
      continue;
    }
    // Superscript / subscript on the *cell* font: drawn smaller and offset,
    // and applied before measuring so the spill scan and alignment use the size
    // actually drawn rather than the nominal one.
    let supShift = 0;
    if (it.sup) {
      const px = Math.max(6, cellPx(it) * 0.72);
      const weight = it.b ? "600 " : "";
      const slant = it.i ? "italic " : "";
      ctx.font = `${slant}${weight}${px}px ${fontStack(it.fn)}`;
      supShift = it.sup === "superscript" ? -cellPx(it) * 0.32 : cellPx(it) * 0.18;
    }
    let text = it.t;
    // Rich text is measured run by run: each has its own font, so measuring
    // the concatenation with the cell's font gives a width that is wrong
    // wherever they differ — and the spill scan below would then borrow the
    // wrong number of neighbouring columns.
    let tw = it.runs ? runsWidth(it) : ctx.measureText(text).width;

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
    // Shrink-to-fit joins `clip` in skipping the spill scan: both mean "stay
    // inside this cell", so borrowing a neighbour first and then shrinking
    // would leave the text scaled down *and* overhanging.
    } else if (!it.cl && !it.shrink && tw > w - 8) {
      if (align !== "right") {
        let c = it.c;
        // Stop at a non-empty cell OR a column that isn't drawn (e.g. the gap
        // between a frozen band and the scrolling body) — colXAt is undefined
        // there and would make the clip NaN.
        // The scan may cross columns that are not drawn: a label whose own
        // cell is left of the window spills *into* it, and stopping at the
        // first undrawn column would stop at the very first step. `fscreen*`
        // has a position for any column, drawn or not. The pane bounds still
        // apply, so a frozen cell cannot borrow the body's columns.
        while (clipR - x < tw + 8 && c + 1 < spillHi && !occupied.has(it.r + "," + (c + 1))) {
          c += 1;
          clipR = fscreenXEnd(c);
        }
      }
      if (align !== "left") {
        let c = it.c;
        // Same reasoning as the rightward scan: a right-aligned label whose
        // cell is off the *right* edge spills back into the window, and the
        // columns it crosses on the way are not drawn either.
        while (x + w - clipL < tw + 8 && c - 1 >= spillLo && !occupied.has(it.r + "," + (c - 1))) {
          c -= 1;
          clipL = fscreenX(c);
        }
      }
    }

    ctx.save();
    if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
    ctx.beginPath();
    ctx.rect(clipL, yTop, clipR - clipL, h);
    ctx.clip();
    // Shrink-to-fit: scale the font down until the text fits its own cell,
    // rather than spilling or clipping. Applied after the spill scan so it
    // overrides it — the whole point of the setting is that the text stays
    // inside, so borrowing a neighbour first would defeat it.
    if (it.shrink && tw > w - 8) {
      const scale = Math.max(0.4, (w - 8) / tw);
      const px = Math.max(5, cellPx(it) * scale);
      const weight = it.b ? "600 " : "";
      const slant = it.i ? "italic " : "";
      ctx.font = `${slant}${weight}${px}px ${fontStack(it.fn)}`;
      tw = ctx.measureText(text).width;
    }
    ctx.fillStyle = cellFg(it);
    let tx;
    const ind = (it.in || 0) * INDENT_PX;
    if (align === "right") { ctx.textAlign = "right"; tx = x + w - 5 - ind; }
    else if (align === "center") { ctx.textAlign = "center"; tx = x + w / 2; }
    else { ctx.textAlign = "left"; tx = x + 5 + ind; }
    if (it.runs && text === it.t) {
      // Rich text: each run carries its own font, so it is drawn piece by
      // piece. Only when the text is the cell's own — a "#####" overflow
      // placeholder has no runs to speak of.
      const total = runsWidth(it);
      const startX = align === "right" ? tx - total : align === "center" ? tx - total / 2 : tx;
      const savedAlign = ctx.textAlign;
      ctx.textAlign = "left";
      drawRuns(it, startX, y);
      ctx.textAlign = savedAlign;
      ctx.restore();
      continue;
    }
    ctx.fillText(text, tx, y + supShift);
    if (it.u || it.st) {
      const lw = Math.min(tw, clipR - clipL - 8);
      const lx = align === "right" ? tx - lw : align === "center" ? tx - lw / 2 : tx;
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = 1;
      if (it.u) {
        ctx.beginPath();
        ctx.moveTo(lx, y + 7.5);
        ctx.lineTo(lx + lw, y + 7.5);
        ctx.stroke();
        // A double or accounting underline is a second rule below the first.
        // Drawing one line for all four kinds is what made a ledger's
        // accounting underline read as an ordinary one.
        if (it.uk === "double" || it.uk === "doubleAccounting") {
          ctx.beginPath();
          ctx.moveTo(lx, y + 10.5);
          ctx.lineTo(lx + lw, y + 10.5);
          ctx.stroke();
        }
      }
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
          ctx.fillStyle = cellFg(it);
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

  // Other participants, drawn after the local selection so a cursor landing on
  // the same cell is still visible, and before the overlays below so it never
  // hides a control the user can click.
  drawCollaborators(v, perQuad);

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
  drawTraceArrows(withQuad);

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

  // The rule's input hint, shown while its cell is selected — Excel's "Input
  // Message". A constraint explained only after you have typed something wrong
  // is explained too late, and the wording was being carried through every save
  // without ever being shown.
  refreshValidationPrompt();

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

  // Hyperlinks: underline them and tint them, which is the only cue that a
  // cell is clickable. Drawn before the comment markers so a cell with both
  // still shows its note triangle on top.
  linkCells = new Set();
  if (wasm) {
    const lr0 = geo.rowIdx[0] ?? state.firstRow, lc0 = geo.colIdx[0] ?? state.firstCol;
    const lr1 = geo.rowIdx[geo.rowIdx.length - 1] ?? lr0;
    const lc1 = geo.colIdx[geo.colIdx.length - 1] ?? lc0;
    let links = [];
    try { links = JSON.parse(wasm.session_hyperlink_cells(state.sheet, lr0, lc0, lr1, lc1)); } catch {}
    for (const lk of links) {
      linkCells.add(lk.r + "," + lk.c);
      const lx = colXAt(lk.c), ly = rowYAt(lk.r);
      if (lx === undefined || ly === undefined) continue;
      const lw = colWAt(lk.c), lh = rowHAt(lk.r);
      withQuad(lk.r, lk.c, () => {
        ctx.strokeStyle = getComputedStyle(ocThemeHost)
          .getPropertyValue("--oc-accent-color").trim() || "#3b82f6";
        ctx.lineWidth = 1;
        ctx.beginPath();
        // Sit the rule on the text baseline rather than the cell floor, or it
        // reads as a bottom border instead of an underline.
        const baseline = ly + lh - Math.max(4, Math.round(lh * 0.22));
        ctx.moveTo(lx + 3, baseline + 0.5);
        ctx.lineTo(lx + lw - 3, baseline + 0.5);
        ctx.stroke();
      });
    }
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
  drawFreezeHandles();

  // The in-cell editor is a DOM element over the canvas: keep it on its cell as
  // the grid scrolls or resizes under it (grid-wrap's overflow clips it once the
  // cell leaves the viewport), instead of leaving it parked mid-air.
  if (editSurface === inline) positionInline();
  updateNameBox();
  announceCell();
  rebuildA11yGrid();
  updateGridCounts();
  updateCellMode();
  updateScrollbars(v);
  updateStats();
  announceCollabSelection();
  if (wasm) refreshFormulaBar();
  if (wasm && activePanel) refreshPanel();
  // Last, so a host's listener sees the state the frame just painted rather
  // than the one it started from.
  if (wasm) emitStateEvents();
}

const FREEZE_GRAB = 4; // px proximity to the freeze divider that arms a drag

// Prominent, draggable freeze dividers (Sheets-style), drawn on top of the
// headers. During a drag the line follows the pointer as a live preview.
function drawFreezeHandles() {
  const F = state.freeze;
  ctx.save();
  ctx.fillStyle = colors.freezeLine;
  ctx.globalAlpha = 0.55;
  if (F.fc === 0) {
    ctx.fillRect(HW - FREEZE_HANDLE + 2, HH * 0.18, FREEZE_HANDLE - 3, HH * 0.44);
  }
  if (F.fr === 0) {
    ctx.fillRect(HW * 0.18, HH - FREEZE_HANDLE + 2, HW * 0.44, FREEZE_HANDLE - 3);
  }
  ctx.restore();
}

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

// Grab handles for *creating* a freeze, in the corner box above the row header.
//
// `freezeHit` below can only find a divider that already exists, because the
// divider is drawn at the freeze line and there is no line at zero. So every
// drag gesture worked on a freeze somebody had already made through the menu,
// and there was no gesture that made one — the affordance a user goes looking
// for first. These are the two handles Sheets puts in the corner: drag the
// right-hand one out to freeze columns, the bottom one down to freeze rows.
//
// They live *inside* the corner box on purpose. The obvious alternative — a
// grab zone on the body's leading edge — sits exactly where column A's cells
// are, and would swallow ordinary selection clicks forever after.
const FREEZE_HANDLE = 7; // px thickness of a corner grab handle
function freezeHandleAt(px, py) {
  if (px >= HW || py >= HH) return null;
  const F = state.freeze;
  if (F.fc === 0 && px >= HW - FREEZE_HANDLE && py <= HH * 0.62) return { axis: "col" };
  if (F.fr === 0 && py >= HH - FREEZE_HANDLE && px <= HW * 0.62) return { axis: "row" };
  return null;
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
  try { wasm.session_set_freeze(state.sheet, fr, fc); } catch (e) { statusError(errText(e)); }
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
  // oc-safe-html: every part is a number this function computed, joined
  // with non-breaking spaces. No workbook text reaches here.
  // oc-safe-html: see the note above.
  selStats.innerHTML = parts.join("&nbsp;&nbsp;&nbsp;");
}

function refreshFormulaBar() {
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
  // Reflect formatting from the selection's top-left (the representative/active
  // cell). For a range/row/column selection state.sel is the *moving end*, which
  // is often an empty corner — reading that left the font/size boxes blank.
  const pr = selRect();
  const fmt = JSON.parse(wasm.session_cell_format(state.sheet, pr.r0, pr.c0));
  const press = (id, on) => byId(id).setAttribute("aria-pressed", on ? "true" : "false");
  press("tb-bold", fmt.b);
  press("tb-italic", fmt.i);
  press("tb-underline", fmt.u);
  press("tb-strike", fmt.st);
  press("tb-wrap", fmt.w || fmt.cl);
  for (const b of qsa(".tb-align")) {
    b.setAttribute("aria-pressed", b.dataset.al === fmt.al ? "true" : "false");
  }
  byId("tb-font").value = fmt.fn || "";
  byId("tb-size").value = fmt.fs ? String(fmt.fs) : "";
}

function cellAt(px, py) {
  if (px < HW || py < HH) return null;
  return { row: rowAtY(py), col: colAtX(px) };
}

// --- Enter / Tab navigation ------------------------------------------------
//
// Two Excel behaviours that both need somewhere to remember a little context.
//
// Inside a multi-cell selection, Enter and Tab walk *within* the block and wrap
// at its edges, leaving the selection itself alone. `state.sel` is the active
// cell everywhere else in this file, and the block is derived from anchor+sel —
// so moving `sel` would shrink the very selection being walked. `navBlock` holds
// the block still while `sel` moves inside it; `selRect` prefers it when set.
//
// And a run of Tabs remembers the column it started in, so Enter returns there
// and drops a row — which is what makes tabbing across a record and pressing
// Enter land at the start of the next one rather than wherever you stopped.
let navBlock = null;   // {r0,c0,r1,c1} being walked, or null
let tabOrigin = null;  // column a Tab run began in

// Any selection change by other means ends both runs.
function resetNavRuns() {
  navBlock = null;
  tabOrigin = null;
}

// The block Enter/Tab should walk, or null for a single cell.
function navTarget() {
  if (navBlock) return navBlock;
  const r = effectiveRange();
  const multi = r.r1 > r.r0 || r.c1 > r.c0;
  return multi ? r : null;
}

// Step the active cell within `b`, wrapping at the edges. `axis` is "row" for
// Enter (down the column, then to the next column) or "col" for Tab.
function stepWithin(b, axis, back) {
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

// Enter: inside a block, walk it; otherwise return to the Tab run's origin
// column and drop a row, or just drop a row.
function enterStep(back) {
  const b = navTarget();
  if (b) { navBlock = b; stepWithin(b, "row", back); return; }
  const col = tabOrigin !== null ? tabOrigin : state.sel.col;
  tabOrigin = null;
  // Merge-aware for the same reason the arrows are: Enter on a vertically
  // merged cell otherwise lands back inside it and the cursor never moves.
  const to = stepFrom(state.sel.row, state.sel.col, back ? -1 : 1, 0);
  select(to.row, col);
}

// Tab: inside a block, walk it; otherwise move sideways, remembering where the
// run began.
function tabStep(back) {
  const b = navTarget();
  if (b) { navBlock = b; stepWithin(b, "col", back); return; }
  if (tabOrigin === null) tabOrigin = state.sel.col;
  const origin = tabOrigin;
  const to = stepFrom(state.sel.row, state.sel.col, 0, back ? -1 : 1);
  select(to.row, to.col);
  tabOrigin = origin; // `select` cleared it; this run is still going
}

/// Step one cell from (row, col) in the direction (dr, dc), treating a merge as
/// the single cell it looks like.
///
/// `select` snaps any coordinate inside a merge back to the merge's top-left
/// anchor, which is right for a click and wrong for a step: arrowing right out
/// of B2:D2 computed (1,2), `select` snapped it back to (1,1), and the selection
/// **never moved**. Left and up worked, because the anchor *is* the top-left, so
/// the failure was asymmetric — which is why it read as a frozen keyboard rather
/// than as a merge rule. Excel treats a merge as one cell: you leave from its
/// far edge and land on the first cell past it.
function stepFrom(row, col, dr, dc) {
  const m = mergeAt(row, col);
  if (!m) return { row: row + dr, col: col + dc };
  // Leave from the edge facing the way we are going, so the landing cell is the
  // first one outside the merge rather than one still inside it.
  const fromRow = dr > 0 ? m.r1 : dr < 0 ? m.r0 : row;
  const fromCol = dc > 0 ? m.c1 : dc < 0 ? m.c0 : col;
  return { row: fromRow + dr, col: fromCol + dc };
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
  extending = false;
  lastFill = null;
  hideFillOptions();
  resetNavRuns();
  ensureVisible();
  draw();
}

// Ctrl/Cmd+click: bank the current range and start a fresh active range at
// (row, col) without clearing the banked ones — builds a multi-range selection.
function addRange(row, col) {
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

// Extend the selection to (row, col), keeping the active cell where it is.
//
// The corner that travels is `state.anchor`, not `state.sel`. That reads
// backwards until you remember that `state.sel` is the **active cell**
// everywhere in this file (UX-NV4) — and in Excel and Sheets the active cell
// stays where the selection began while the far corner follows the keyboard or
// the pointer. Moving `sel` instead put the active cell at the end of the
// travel, so selecting B2:B4 with Shift+Down and typing wrote into B4: the
// value landed in the last cell the user passed over rather than the one the
// selection was still highlighting.
//
// `selRect` takes the min and max of the two, so which end moves makes no
// difference to the block; it decides only where the active cell ends up.
function extend(row, col) {
  state.anchor = { row: Math.max(0, row), col: Math.max(0, col) };
  state.selKind = "cells";
  extending = true;
  // Follow the travelling corner, not the active cell, which is not moving.
  ensureVisible(state.anchor.row, state.anchor.col);
  draw();
}

// Scroll the body just enough to show a cell — the active one by default, or
// any cell (arrow-key point mode follows the cell it is pointing at, which is
// not the selection).
/// Round a scroll offset up to the next whole column / row boundary.
///
/// Scrolling a cell into view by the *bare minimum* puts its far edge against
/// the far edge of the viewport, which leaves the **leading** edge somewhere in
/// the middle of whatever column or row happens to be there. The remainder is a
/// different size for every target, so arrowing across a sheet makes the first
/// visible column look like it is growing and shrinking — the grid appears to be
/// resizing itself rather than scrolling, and scrolling back makes it "expand"
/// again (UX-GRID-02). Excel never shows a partial leading column: it scrolls
/// until that column is whole, and so does this.
///
/// Snapping *up* only ever scrolls further in the direction already being
/// travelled, so the cell that prompted the scroll stays on screen. `limit` is
/// its own leading edge — itself a boundary — and is both the furthest this may
/// go and the answer when the cell is wider or taller than the viewport, where
/// no aligned offset can show it and showing it wins over aligning.
///
/// Rows go through `rowAtPx`/`rowOffsetPx` rather than the engine directly,
/// because a row grown by wrapped or rotated text is taller than the engine
/// thinks and those two are the pair that knows it.
/// `px` and `limit` are body-space (what `state.scrollX`/`scrollY` hold), while
/// the engine indexes absolute sheet pixels — hence `frozen`, the width or
/// height of the frozen band, added on the way in and taken off on the way out.
/// Getting that conversion wrong would misalign only on sheets with a frozen
/// pane, which is exactly the sort of thing that survives a demo.
function snapLeading(px, limit, frozen, isCol) {
  if (px >= limit) return limit;
  const abs = Math.max(0, px + frozen);
  const at = isCol ? wasm.session_col_at_px(state.sheet, Math.round(abs)) : rowAtPx(abs);
  const startOf = (i) =>
    (isCol ? wasm.session_col_offset_px(state.sheet, i) : rowOffsetPx(i)) - frozen;
  const start = startOf(at);
  return Math.min(start >= px ? start : startOf(at + 1), limit);
}

/// How the engine says a data bar is drawn.
///
/// Read once: it is a set of constants compiled into the engine, so asking per
/// frame would cross the WebAssembly boundary sixty times a second to be told
/// the same thing. The fallback matches the engine's own values and exists so a
/// build predating `session_data_bar_style` still draws bars rather than
/// throwing in the paint loop.
let dataBarStyleCache = null;
function dataBarStyle() {
  if (dataBarStyleCache) return dataBarStyleCache;
  try {
    dataBarStyleCache = JSON.parse(wasm.session_data_bar_style());
  } catch {
    dataBarStyleCache = { padX: 1, padY: 2, alpha: 0.45, defaultColor: "638EC6" };
  }
  return dataBarStyleCache;
}

/// Arm or disarm End mode, and say so.
///
/// Announced in the status bar because an armed mode with no indicator is a
/// keystroke that behaves differently for reasons the user cannot see — Excel
/// shows "End Mode" for exactly that reason.
function setEndMode(on) {
  if (state.endMode === on) return;
  state.endMode = on;
  if (status) status.textContent = on ? "End mode — press an arrow key" : "";
}

function ensureVisible(row = state.sel.row, col = state.sel.col) {
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
function commit(value, advance, source = "user") {
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
// Progressive Ctrl+A (Excel): the first press selects the contiguous block of
// data around the cursor — the table you are standing in — the second widens to
// the whole used region, and the third to the entire sheet.
//
// It used to jump straight to the used region from A1, which on a loaded sheet
// reads as "select everything" and loses the one selection you actually wanted:
// this block. A blank cursor cell has no block, so it starts at the used region.
function ctrlA() {
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
// Double-click a row boundary: size the row to its tallest cell, honoring each
// cell's font size, wrap (wrapped to the column width), and explicit newlines.
function autofitRow(row) {
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

// Run a formatting op over the whole selection (every range), then redraw.
function formatSel(fn) {
  try { for (const s of allRanges()) fn(s); } catch (e) { statusError(errText(e)); }
  draw();
}
function toggleBold() { formatSel((s) => wasm.session_toggle_bold(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleItalic() { formatSel((s) => wasm.session_toggle_italic(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleUnderline() { formatSel((s) => wasm.session_toggle_underline(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
function toggleStrike() { formatSel((s) => wasm.session_toggle_strike(state.sheet, s.r0, s.c0, s.r1, s.c1)); }
// `link` is an optional {slot, tint} from the theme row; -1 means "not from the
// theme", which is how the engine tells a themed colour from a literal one.
function setFill(hex, link) {
  formatSel((s) => wasm.session_set_fill(
    state.sheet, s.r0, s.c0, s.r1, s.c1, hex, link ? link.slot : -1, link ? link.tint : 0));
}
function setFontColor(hex, link) {
  formatSel((s) => wasm.session_set_font_color(
    state.sheet, s.r0, s.c0, s.r1, s.c1, hex, link ? link.slot : -1, link ? link.tint : 0));
}

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
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Conditional formatting rules";

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

// Format Cells (Ctrl+1): the number/font/alignment/fill/border controls in one
// place. The toolbar has all of these, but scattered — this is the dialog people
// reach for when they want to set several at once and see them together.
function formatCellsDialog() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Format cells";
  body.textContent = "";

  let cur = {};
  try { cur = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col)) || {}; }
  catch {}

  const tabs = el("div", "fc-tabs");
  const pages = el("div", "fc-pages");
  const made = [];
  const addTab = (name, build) => {
    const b = el("button", "fc-tab", name);
    const page = el("div", "fc-page");
    page.hidden = made.length > 0;
    if (!made.length) b.classList.add("on");
    build(page);
    b.addEventListener("click", () => {
      for (const [tb, pg] of made) { tb.classList.remove("on"); pg.hidden = true; }
      b.classList.add("on");
      page.hidden = false;
    });
    tabs.appendChild(b);
    pages.appendChild(page);
    made.push([b, page]);
  };

  // Each page collects its own setter, applied together on OK so the whole
  // dialog is one visible change rather than a dozen.
  const pending = [];

  addTab("Number", (page) => {
    page.append(el("p", "oc-confirm-text", "Format code"));
    const inp = el("input", "cf-code");
    inp.value = cur.nf || "";
    inp.placeholder = "General";
    inp.spellcheck = false;
    const preview = el("div", "cf-preview");
    const render = () => {
      try { preview.textContent = inp.value.trim() ? wasm.format_preview(1234.567, inp.value.trim()) : "1234.567"; }
      catch { preview.textContent = "—"; }
    };
    inp.addEventListener("input", render);
    render();
    const presets = el("div", "cf-presets");
    for (const [label, code] of [
      ["General", ""], ["0.00", "0.00"], ["#,##0", "#,##0"], ["0%", "0%"],
      ["$#,##0.00", "$#,##0.00"], ["yyyy-mm-dd", "yyyy-mm-dd"], ["Text", "@"],
    ]) {
      const b = el("button", "cf-preset", label);
      b.addEventListener("click", () => { inp.value = code; render(); });
      presets.appendChild(b);
    }
    page.append(inp, preview, presets);
    pending.push((s) => wasm.session_set_number_format(state.sheet, s.r0, s.c0, s.r1, s.c1, inp.value.trim()));
  });

  addTab("Font", (page) => {
    const row = el("div", "fc-row");
    const mk = (label, on) => {
      const l = el("label", "fc-check");
      const c = document.createElement("input");
      c.type = "checkbox";
      c.checked = !!on;
      l.append(c, document.createTextNode(" " + label));
      row.appendChild(l);
      return c;
    };
    const b = mk("Bold", cur.b), i = mk("Italic", cur.i);
    const u = mk("Underline", cur.u), st = mk("Strikethrough", cur.st);
    page.append(row);
    page.append(el("p", "oc-confirm-text", "Size (pt)"));
    const size = el("input", "panel-field");
    size.type = "number"; size.min = "1"; size.max = "409";
    size.value = cur.fs || "";
    size.placeholder = "default";
    page.append(size);
    page.append(el("p", "oc-confirm-text", "Text colour"));
    const col = document.createElement("input");
    col.type = "color";
    col.value = cur.fc ? "#" + cur.fc : "#000000";
    page.append(col);
    pending.push((s) => {
      wasm.session_set_font_flags(
        state.sheet, s.r0, s.c0, s.r1, s.c1,
        b.checked, i.checked, u.checked, st.checked);
      if (size.value) wasm.session_set_font_size(state.sheet, s.r0, s.c0, s.r1, s.c1, parseFloat(size.value));
      wasm.session_set_font_color(state.sheet, s.r0, s.c0, s.r1, s.c1, col.value.replace("#", ""), -1, 0);
    });
  });

  addTab("Alignment", (page) => {
    page.append(el("p", "oc-confirm-text", "Horizontal"));
    const h = el("select", "panel-select");
    for (const [v, t] of [["", "General"], ["left", "Left"], ["center", "Center"], ["right", "Right"],
                          ["fill", "Fill"], ["justify", "Justify"],
                          ["centerContinuous", "Center across selection"], ["distributed", "Distributed"]]) {
      const o = el("option", null, t); o.value = v; h.appendChild(o);
    }
    h.value = cur.al || "";
    const v = el("select", "panel-select");
    for (const [val, t] of [["", "Default"], ["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"],
                            ["justify", "Justify"], ["distributed", "Distributed"]]) {
      const o = el("option", null, t); o.value = val; v.appendChild(o);
    }
    v.value = { t: "top", m: "middle", b: "bottom", vj: "justify", vd: "distributed" }[cur.va] || "";
    const wrapL = el("label", "fc-check");
    const wrapC = document.createElement("input");
    wrapC.type = "checkbox"; wrapC.checked = !!cur.w;
    wrapL.append(wrapC, document.createTextNode(" Wrap text"));
    page.append(el("p", "oc-confirm-text", "Horizontal"), h,
                el("p", "oc-confirm-text", "Vertical"), v, wrapL);
    pending.push((s) => {
      wasm.session_set_align(state.sheet, s.r0, s.c0, s.r1, s.c1, h.value);
      wasm.session_set_valign(state.sheet, s.r0, s.c0, s.r1, s.c1, v.value);
      wasm.session_set_text_overflow(state.sheet, s.r0, s.c0, s.r1, s.c1, wrapC.checked ? "wrap" : "overflow");
    });
  });

  addTab("Fill", (page) => {
    page.append(el("p", "oc-confirm-text", "Background"));
    const col = document.createElement("input");
    col.type = "color";
    col.value = cur.bg ? "#" + cur.bg : "#ffffff";
    const none = el("button", "cf-preset", "No fill");
    let cleared = false;
    none.addEventListener("click", () => { cleared = true; none.classList.add("on"); });
    col.addEventListener("input", () => { cleared = false; none.classList.remove("on"); });
    page.append(col, none);
    pending.push((s) =>
      wasm.session_set_fill(state.sheet, s.r0, s.c0, s.r1, s.c1, cleared ? "" : col.value.replace("#", ""), -1, 0));
  });

  addTab("Border", (page) => {
    page.append(el("p", "oc-confirm-text", "Placement"));
    const grid = el("div", "fc-borders");
    let chosen = null;
    for (const kind of ["all", "outer", "inner", "top", "bottom", "left", "right",
                        "topandbottom", "bottomdouble", "diagdown", "diagup", "none"]) {
      const b = el("button", "cf-preset", BD_TITLES[kind] || kind);
      b.addEventListener("click", () => {
        chosen = kind;
        grid.querySelectorAll("button").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
      });
      grid.appendChild(b);
    }
    page.append(grid);
    page.append(el("div", "panel-hint", "Uses the line style and colour from the toolbar's border palette."));
    pending.push((s) => {
      if (chosen) wasm.session_set_border(state.sheet, s.r0, s.c0, s.r1, s.c1, chosen, borderStyle, borderColor);
    });
  });

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Apply");
  actions.append(cancel, ok);
  body.append(tabs, pages, actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    const s = effectiveRange();
    close();
    canvas.focus();
    // One try around the lot: a failure part-way through should report once, not
    // once per tab.
    try { for (const apply of pending) apply(s); }
    catch (e) { statusError(errText(e)); }
    draw();
  });
  ok.focus();
}

// The named cell-style gallery. Applying one writes its formatting *and*
// records which style the cells belong to, so the association survives a save —
// that link is the whole point of a named style over ad-hoc formatting.
function cellStyleGallery() {
  let styles = [];
  try { styles = JSON.parse(wasm.session_cell_styles()); } catch {}
  if (!styles.length) { status.textContent = "no cell styles available"; return; }

  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Cell styles";
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
  // `link` is the theme slot + tint a swatch came from, or null for a colour
  // with no theme behind it. Carrying it through to the model is what lets a
  // themed cell follow the workbook when the palette is changed elsewhere; a
  // theme swatch stored as bare RRGGBB is indistinguishable from a hand-picked
  // colour and stays put forever.
  const pick = (hex, link) => { pushRecent(hex); onPick(hex, link || null); menu.hidden = true; canvas.focus(); };
  const none = el("button", "cm-none");
  // oc-safe-html: a literal SVG icon plus `noneLabel`, which is a UI string
  // from this module or the host's i18n table — never workbook text.
  // oc-safe-html: see the note above.
  none.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="icon-sm"><circle cx="12" cy="12" r="9"/><line x1="5.6" y1="5.6" x2="18.4" y2="18.4"/></svg>' +
    `<span>${noneLabel}</span>`;
  none.addEventListener("click", (e) => { e.stopPropagation(); pick(""); });
  menu.appendChild(none);

  // `links[i]`, when given, is the theme slot + tint that produced `colors[i]`.
  const grid = (colors, links) => {
    const g = el("div", "cm-grid");
    colors.forEach((c, i) => {
      const b = el("button", "cm-sw");
      b.style.background = "#" + c;
      b.title = "#" + c;
      const link = links && links[i];
      b.addEventListener("click", (e) => { e.stopPropagation(); pick(c, link); });
      g.appendChild(b);
    });
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
    const slots = order.filter((i) => theme[i]);
    const base = slots.map((i) => theme[i]);
    menu.appendChild(el("div", "cm-label", "Theme"));
    menu.appendChild(grid(base, slots.map((slot) => ({ slot, tint: 0 }))));
    // Excel's tint ladder under the base row: lighter above, darker below. A
    // tinted swatch stays linked to its slot — the tint is part of the
    // reference, not a way out of it.
    for (const t of [0.6, 0.4, -0.25, -0.5]) {
      menu.appendChild(grid(
        base.map((c) => tintColor(c, t)),
        slots.map((slot) => ({ slot, tint: t })),
      ));
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
// Flip `locked` / `hidden` over the selection. Both are style bits, so this
// goes through the same undoable range-styling path as bold.
function setCellProtection(which) {
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

// Whether the current sheet is protected, as the engine holds it.
function sheetProtectedNow() {
  try { return !!JSON.parse(wasm.session_sheet_protected())[state.sheet]; }
  catch { return false; }
}
function toggleSheetProtected() {
  const now = sheetProtectedNow();
  tryEdit(() => wasm.session_set_sheet_protected(state.sheet, !now));
  status.textContent = now ? "sheet unprotected" : "sheet protected — locked cells refuse edits";
}

// Hand the engine the wall clock and, optionally, a fresh random seed.
//
// The engine deliberately reads no clock of its own, so nothing volatile works
// until this has run. Called once at startup and again on every explicit
// recalculation; `reseed` is what makes RAND reroll rather than repeat.
let volatileSeed = 1;
function syncClock(reseed = false) {
  if (!wasm) return;
  if (reseed) volatileSeed = (volatileSeed * 1103515245 + 12345) >>> 0;
  const now = new Date();
  // Excel's epoch is 1899-12-30, and the serial is local time, not UTC — a
  // UTC serial puts TODAY() on the wrong day for most of the world's evening.
  const local = new Date(now.getTime() - now.getTimezoneOffset() * 60000);
  const serial = local.getTime() / 86400000 + 25569;
  // Swallowed: no user asked for this. It runs at boot and on every recalc,
  // before a session may even exist, and there is no control here whose failure
  // a message would explain.
  try { wasm.session_set_clock(serial, volatileSeed); } catch {}
}

// The sheet's display switches, as the engine holds them.
function viewOptions() {
  try { return JSON.parse(wasm.session_view_options(state.sheet)); }
  catch { return { formulas: false, zeros: true }; }
}
// Flip one of them. Undoable like any other sheet-level change.
function setViewOption(which) {
  const now = !!viewOptions()[which];
  tryEdit(() => wasm.session_set_view_option(state.sheet, which, !now));
}
// Subscript / superscript. Pressing the one already applied turns it off.
function setVertAlign(which) { formatSel((s) => wasm.session_toggle_vert_align(state.sheet, s.r0, s.c0, s.r1, s.c1, which)); }
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
  const btn = byId("tb-painter");
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
  } catch (e) { statusError(errText(e)); }
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
  } catch (e) { statusError(errText(e)); }
  draw();
}

// The custom number-format dialog. The engine understands far more codes than
// the preset menu offers — scientific, section colours, a text section — and
// without somewhere to type one, none of that is reachable. Previews against
// the active cell's own value, so you can see what the code does to *your*
// data before applying it.
function customFormatDialog() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Custom number format";
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
  } catch (e) { statusError(errText(e)); }
  draw();
}

// The Sort dialog: choose up to three keys and say whether row 1 is a heading.
// The single-click A→Z / Z→A menu items stay for the common case; this is for
// when "sort by region, then by total descending" is what you actually meant.
function sortDialog() {
  const s = sortTarget();
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Sort range";
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
let filterInfo = null;    // the *sheet's* own filter: {r0,c0,r1,c1,cols:Set<absCol>,hidden} or null
let filterRegions = [];   // every filter on the sheet, tables included
let filterHidden = 0;     // rows hidden by all of them together
let filterButtons = [];   // hit targets rebuilt each frame by drawFilterButtons()

function refreshFilterInfo() {
  filterInfo = null;
  filterRegions = [];
  filterHidden = 0;
  if (!wasm) return;
  try {
    const j = wasm.session_filter_info(state.sheet);
    if (j && j !== "null") {
      filterInfo = JSON.parse(j);
      filterInfo.cols = new Set(filterInfo.cols);
    }
  } catch {}
  try {
    // Regions come from the model rather than from table geometry, so a table
    // column carrying a rule draws as filtered like any other.
    const payload = JSON.parse(wasm.session_filter_regions(state.sheet));
    filterHidden = payload.hidden || 0;
    filterRegions = payload.regions.map((r) => ({ ...r, cols: new Set(r.cols) }));
  } catch {}
}

// Hide the rows a value-set excludes, for this participant only (COL-32).
//
// The engine computes *which* rows so that the personal and shared paths cannot
// disagree about the same tick-boxes; only where the answer is stored differs.
function applyPersonalFilter(col, values) {
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

// Drop every personal view. A first-class command because undo will not do it:
// a personal view is not a document edit, so undo reverses the last change to
// the *document* instead.
function clearMyView() {
  if (!wasm) return;
  try {
    wasm.session_clear_all_personal_views();
    status.textContent = "your view cleared — showing every row";
    afterFilterChange();
  } catch (why) {
    console.error("[opencalc] clear view", why);
  }
}

// Turn the filter on over the current block, or off if one is already on.
function toggleFilter() {
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

// Draw a dropdown button on each header cell in the filter range, recording the
// hit targets. A column carrying a rule gets the accent treatment so it is
// obvious at a glance which columns are narrowing the view.
//
// `withQuad` is draw()'s pane clipper, passed in because it closes over the
// frame's frozen-pane geometry: a button on a header scrolled under a frozen
// pane must be clipped to its own pane, not painted over the frozen one.
function drawFilterButtons(withQuad) {
  filterButtons = [];
  // Every header that carries filter buttons: the sheet's own autofilter, plus
  // each table's. A table brings its own — Excel turns them on with the table,
  // and without them a table header reads as an ordinary shaded row.
  for (const region of filterRegions) drawFilterRegion(withQuad, region);
}

function drawFilterRegion(withQuad, filterInfo) {
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

  // Whose filter is this? docs/71: the choice is offered, and defaults to
  // shared, because shared is what a spreadsheet has always done and the only
  // one the file format can express. "Just for me" never touches the document —
  // no operation on the wire, nothing in the undo history, nothing saved, and
  // the SUBTOTAL underneath does not move.
  const scope = document.createElement("label");
  scope.className = "filter-scope";
  const mine = document.createElement("input");
  mine.type = "checkbox";
  mine.className = "filter-scope-box";
  scope.appendChild(mine);
  scope.appendChild(document.createTextNode(" Just for me"));
  const scopeHint = document.createElement("div");
  scopeHint.className = "panel-hint";
  scopeHint.textContent = "Others keep seeing every row.";
  scopeHint.hidden = true;
  mine.addEventListener("change", () => { scopeHint.hidden = !mine.checked; });
  menu.appendChild(scope);
  menu.appendChild(scopeHint);

  const foot = document.createElement("div");
  foot.className = "filter-foot";
  const clr = document.createElement("button");
  clr.className = "filter-clear";
  clr.textContent = "Clear";
  clr.addEventListener("click", () => {
    closeSheetMenu();
    // Clearing drops this participant's view *and* the shared rule, because
    // "Clear" on a column means the column is not filtering — and a user who
    // cannot tell which of the two hid a row cannot be asked to clear the right
    // one.
    if (wasm.session_has_personal_view(state.sheet)) {
      try { wasm.session_clear_personal_view(state.sheet); } catch {}
    }
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
    if (mine.checked) {
      // Personal: ask the engine which rows this value-set hides, then keep
      // them in the session's own view. Deliberately *not* `tryEdit` — this is
      // not an edit, and routing it through one is the mistake docs/71 exists
      // to prevent.
      applyPersonalFilter(col, values);
    } else {
      tryEdit(() => wasm.session_set_filter_values(state.sheet, col, values));
    }
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
  // Counted across every filter, not just the sheet's own: a table carries its
  // own, and reading `filterInfo` alone reported "filter cleared" on the edit
  // that had just hidden rows.
  const n = filterHidden;
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

  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Filter by condition";
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
      catch (e) { statusError(errText(e)); }
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
let activePanel = null;        // 'dv' | 'cf' | 'note' | 'table' | 'page' | null
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

// The table panel: name, style and the six banding switches, all applied the
// moment you change them.
//
// Every control here is one `session_*` call and therefore one undo step — the
// panel holds no state of its own, so it cannot disagree with the workbook.
function buildTablePanel(body) {
  const t = currentTable();
  if (!t) {
    body.appendChild(el("div", "panel-note", "Select a cell inside a table."));
    return;
  }
  const at = () => ({ r: t.r0, c: t.c0 });

  panelLabel(body, "Name");
  const name = el("input", "panel-field");
  name.type = "text";
  name.value = t.name;
  // On commit rather than per keystroke: a half-typed name is usually invalid,
  // and rejecting it mid-word would fight the person typing.
  const rename = () => {
    const want = name.value.trim();
    if (!want || want === t.name) { name.value = t.name; return; }
    try {
      wasm.session_rename_table(state.sheet, at().r, at().c, want);
      status.textContent = `renamed to ${want}`;
    } catch (e) {
      statusError(errText(e));
      name.value = t.name;
    }
    draw();
    refreshTablePanel();
  };
  name.addEventListener("change", rename);
  name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.stopPropagation(); name.blur(); }
    else if (e.key === "Escape") { e.stopPropagation(); name.value = t.name; name.blur(); }
  });
  body.appendChild(name);

  panelLabel(body, "Range");
  body.appendChild(el("div", "panel-range", `${A1(t.r0, t.c0)}:${A1(t.r1, t.c1)}`));

  panelLabel(body, "Style");
  const styles = el("div", "oc-table-styles");
  for (const [label, id] of TABLE_STYLES) {
    const b = el("button", "oc-style-swatch" + (id === t.style ? " sel" : ""));
    b.type = "button";
    b.title = label;
    // Swatches are painted from the engine's own resolution, so the preview
    // and the grid cannot disagree about what a style looks like.
    let c = { headerFill: "FFFFFF", bandFill: "F2F2F2", border: "BFBFBF" };
    try { c = JSON.parse(wasm.session_table_style_preview(id)) || c; } catch {}
    const head = el("span");
    head.style.background = "#" + c.headerFill;
    head.style.borderBottom = "2px solid #" + c.border;
    const band = el("span");
    band.style.background = "#" + c.bandFill;
    b.append(head, el("span"), band, el("span"));
    b.addEventListener("click", () => applyTableStyle({ style: id }));
    styles.appendChild(b);
  }
  body.appendChild(styles);

  panelLabel(body, "Show");
  const checks = el("div", "oc-table-checks");
  const check = (label, on, onChange) => {
    const l = el("label", "oc-check");
    const i = document.createElement("input");
    i.type = "checkbox";
    i.checked = on;
    i.addEventListener("change", () => onChange(i.checked));
    l.append(i, document.createTextNode(" " + label));
    checks.appendChild(l);
  };
  check("Header row", t.headers > 0, (on) => {
    tryEdit(() => wasm.session_set_table_headers(state.sheet, at().r, at().c, on));
    refreshTablePanel();
  });
  check("Totals row", t.totals > 0, (on) => {
    tryEdit(() => wasm.session_table_totals(state.sheet, at().r, at().c, on));
    refreshTablePanel();
  });
  check("Banded rows", !!t.stripes, (on) => applyTableStyle({ stripes: on }));
  check("Banded columns", !!t.colStripes, (on) => applyTableStyle({ colStripes: on }));
  check("First column", !!t.firstCol, (on) => applyTableStyle({ firstCol: on }));
  check("Last column", !!t.lastCol, (on) => applyTableStyle({ lastCol: on }));
  body.appendChild(checks);

  // One picker per column, only while there is a totals row to put a result
  // in. Choosing a function writes the SUBTOTAL the choice means — recording
  // the choice alone leaves the row blank here and in Excel.
  if (t.totals > 0) {
    panelLabel(body, "Totals row");
    let funcs = [];
    try {
      funcs = JSON.parse(wasm.session_totals_functions(state.sheet, at().r, at().c));
    } catch {}
    const grid = el("div", "oc-totals-grid");
    for (let c = t.c0; c <= t.c1; c++) {
      const i = c - t.c0;
      const lab = el("span", "oc-totals-col", (t.cols && t.cols[i]) || A1(t.r0, c));
      const sel = el("select", "panel-select");
      for (const [label, id] of TOTALS_FUNCTIONS) {
        const o = document.createElement("option");
        o.value = id;
        o.textContent = label;
        if (id === (funcs[i] || "")) o.selected = true;
        sel.appendChild(o);
      }
      sel.addEventListener("change", () => {
        tryEdit(() => wasm.session_set_totals_function(state.sheet, t.r1, c, sel.value));
        refreshTablePanel();
      });
      grid.append(lab, sel);
    }
    body.appendChild(grid);
  }

  const row = el("div", "panel-actions");
  const rm = el("button", "panel-btn-ghost", "Convert to range");
  rm.addEventListener("click", async () => {
    const cur = currentTable();
    if (!cur) return;
    const ok = await confirmModal(
      `Convert "${cur.name}" to a range`,
      "The values and formatting stay. The table's name goes, so any formula "
        + "written as " + cur.name + "[Column] will stop resolving.",
      "Convert to range",
    );
    if (!ok) return;
    tryEdit(() => wasm.session_remove_table(state.sheet, cur.r0, cur.c0));
    status.textContent = "converted to a range";
    closePanel();
  });
  row.appendChild(rm);
  body.appendChild(row);
}

// The table under the cursor, as the engine reports it, or null.
function currentTable() {
  try {
    return JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col));
  } catch { return null; }
}

// Write the style name and banding flags. Anything not named keeps its current
// value, so a single checkbox does not silently reset the other five.
function applyTableStyle(change) {
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

// Rebuild the panel from the workbook after any change, so what it shows is
// what the model holds rather than what the last click intended.
function refreshTablePanel() {
  if (activePanel !== "table") return;
  const body = byId("side-panel-body");
  body.textContent = "";
  buildTablePanel(body);
}

// --- Chart panel -----------------------------------------------------------
//
// Which chart is being edited is remembered rather than looked up from the
// cursor: a chart floats over cells rather than occupying them, so the
// selection is not where it is.
let panelChart = null;

const CHART_KINDS = [
  ["column", "Column"], ["bar", "Bar"], ["line", "Line"],
  ["area", "Area"], ["pie", "Pie"], ["doughnut", "Doughnut"], ["scatter", "Scatter"],
];

const LEGEND_POSITIONS = [
  ["", "None"], ["r", "Right"], ["b", "Bottom"], ["t", "Top"], ["l", "Left"],
];

function chartAt(row, col) {
  try { return JSON.parse(wasm.session_chart_at(state.sheet, row, col)); } catch { return null; }
}

function currentChart() {
  if (!panelChart) return null;
  try {
    return JSON.parse(wasm.session_chart_defs(panelChart.sheet))[panelChart.index] || null;
  } catch { return null; }
}

function applyChart(c) {
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

function refreshChartPanel() {
  if (activePanel !== "chart") return;
  const body = byId("side-panel-body");
  body.textContent = "";
  buildChartPanel(body);
}

function buildChartPanel(body) {
  const c = currentChart();
  if (!c) {
    body.appendChild(el("div", "panel-note", "Select a chart, or Insert ▸ Chart."));
    return;
  }
  if (c.imported) {
    body.appendChild(el("div", "panel-note",
      "From the file. Changing anything here replaces the chart definition Excel "
      + `saved, and the formatting ${BRAND} does not model goes with it.`));
  }

  panelLabel(body, "Type");
  const kind = el("select", "panel-select");
  for (const [value, label] of CHART_KINDS) {
    const o = el("option", null, label);
    o.value = value;
    kind.appendChild(o);
  }
  kind.value = c.kind;
  kind.addEventListener("change", () => { c.kind = kind.value; applyChart(c); });
  body.appendChild(kind);

  const textField = (label, get, set) => {
    panelLabel(body, label);
    const input = el("input", "panel-field");
    input.type = "text";
    input.value = get();
    const commit = () => { set(input.value); applyChart(c); };
    input.addEventListener("change", commit);
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.stopPropagation(); input.blur(); }
      else if (e.key === "Escape") { e.stopPropagation(); input.value = get(); input.blur(); }
    });
    body.appendChild(input);
  };
  textField("Title", () => c.title, (v) => { c.title = v; });
  textField("Horizontal axis title", () => c.xTitle, (v) => { c.xTitle = v; });
  textField("Vertical axis title", () => c.yTitle, (v) => { c.yTitle = v; });

  panelLabel(body, "Legend");
  const legend = el("select", "panel-select");
  for (const [value, label] of LEGEND_POSITIONS) {
    const o = el("option", null, label);
    o.value = value;
    legend.appendChild(o);
  }
  legend.value = c.legend || "";
  legend.addEventListener("change", () => {
    c.legend = legend.value || null;
    applyChart(c);
  });
  body.appendChild(legend);

  panelLabel(body, "Series");
  c.series.forEach((s, i) => {
    const row = el("div", "chart-series");
    const name = el("input", "panel-field");
    name.type = "text";
    name.placeholder = "name";
    name.value = s.name;
    name.addEventListener("change", () => { s.name = name.value; applyChart(c); });
    const values = el("input", "panel-field");
    values.type = "text";
    values.placeholder = "Sheet1!$B$2:$B$9";
    values.value = s.values;
    values.addEventListener("change", () => { s.values = values.value; applyChart(c); });
    const remove = el("button", "pivot-mini pivot-remove", "✕");
    remove.title = "Remove this series";
    remove.addEventListener("click", () => {
      // The last one goes with the chart: a chart plotting nothing is a blank
      // rectangle nobody can tell from a bug.
      if (c.series.length === 1) {
        statusError("a chart needs at least one series — delete the chart instead");
        return;
      }
      c.series.splice(i, 1);
      applyChart(c);
    });
    const head = el("div", "chart-series-head");
    head.appendChild(name);
    head.appendChild(remove);
    row.appendChild(head);
    row.appendChild(values);
    body.appendChild(row);
  });

  const add = el("button", "panel-btn-ghost", "Add series from selection");
  add.addEventListener("click", () => {
    const r = effectiveRange();
    const sheetName = sheetNameAt(state.sheet);
    c.series.push({
      name: "",
      categories: c.series[0] ? c.series[0].categories : null,
      values: `${/^[A-Za-z_][A-Za-z0-9_.]*$/.test(sheetName) ? sheetName : `'${sheetName.replace(/'/g, "''")}'`}!$${colName(r.c0)}$${r.r0 + 1}:$${colName(r.c1)}$${r.r1 + 1}`,
    });
    applyChart(c);
  });
  body.appendChild(add);

  panelLabel(body, "Category labels");
  const cats = el("input", "panel-field");
  cats.type = "text";
  cats.placeholder = "Sheet1!$A$2:$A$9";
  cats.value = c.series[0]?.categories || "";
  cats.addEventListener("change", () => {
    // One category reference for the whole chart: OOXML lets each series carry
    // its own, but a chart whose series are labelled differently plots points
    // that do not line up, which is a mistake rather than a feature.
    for (const s of c.series) s.categories = cats.value || null;
    applyChart(c);
  });
  body.appendChild(cats);

  panelActions(body, "Move here", () => {
    const r = effectiveRange();
    c.anchor = [r.r0, r.c0, Math.max(r.r1, r.r0 + 8), Math.max(r.c1, r.c0 + 4)];
    applyChart(c);
  }, "Delete", async () => {
    if (!await confirmModal("Delete chart", "Delete this chart?", "Delete")) return;
    tryEdit(() => wasm.session_delete_chart(panelChart.sheet, panelChart.index));
    panelChart = null;
    refreshChartPanel();
  });
}

// Where each chart was last painted, in canvas pixels, plus which one is
// selected.
//
// Exposed because a chart is drawn on a canvas and has no DOM node: without
// this there is no way for an automated check — or a screen reader shim, or a
// screenshot differ — to say *where* a chart is, and verifying drag and resize
// means guessing at coordinates and reading the answer off a screenshot. It is
// a read-only view of state the renderer already computed.
window.openCalcChartFrames = () => ({
  frames: chartFrames.map((f) => ({ ...f })),
  selected: chartSel && chartSel.sheet === state.sheet ? chartSel.index : null,
  handles: chartSel
    ? chartHandlePoints(chartFrames.find((f) => f.index === chartSel.index) || { x: 0, y: 0, w: 0, h: 0 })
    : [],
});

// A press on a chart or one of its handles. Returns whether it was consumed.
function chartMouseDown(px, py) {
  // A handle on the already-selected chart wins over the frame under it: the
  // handles sit on the border, so a corner is inside both.
  if (chartSel && chartSel.sheet === state.sheet) {
    const frame = chartFrames.find((f) => f.index === chartSel.index);
    if (frame) {
      const points = chartHandlePoints(frame);
      for (let i = 0; i < points.length; i++) {
        const [hx, hy] = points[i];
        if (Math.abs(px - hx) <= CHART_HANDLE + 3 && Math.abs(py - hy) <= CHART_HANDLE + 3) {
          chartDrag = { index: chartSel.index, handle: i, x0: px, y0: py, px, py };
          return true;
        }
      }
    }
  }
  // Topmost first: charts overlap, and the one drawn last is the one you see.
  for (let i = chartFrames.length - 1; i >= 0; i--) {
    const f = chartFrames[i];
    if (px >= f.x && px <= f.x + f.w && py >= f.y && py <= f.y + f.h) {
      chartSel = { sheet: state.sheet, index: f.index };
      panelChart = { ...chartSel };
      chartDrag = { index: f.index, handle: null, x0: px, y0: py, px, py };
      if (activePanel === "chart") refreshChartPanel();
      draw();
      return true;
    }
  }
  if (chartSel) { chartSel = null; draw(); }
  return false;
}

// Commit a move or resize: pixels become cells, because a chart is anchored in
// cells and has to move with the rows under it.
function chartMouseUp() {
  if (!chartDrag) return;
  const rect = chartDragRect();
  const moved = Math.abs(chartDrag.px - chartDrag.x0) > 2
    || Math.abs(chartDrag.py - chartDrag.y0) > 2;
  const index = chartDrag.index;
  chartDrag = null;
  if (!rect || !moved) { draw(); return; }
  // Cell **plus offset**, not the nearest cell. Snapping to gridlines is why a
  // drag appeared to do nothing until it crossed one and then jumped a whole
  // column, and why a chart never came to rest where it was dropped.
  const from = anchorPoint(rect.x, rect.y);
  // The far corner is measured past the trailing edge of the cell it lands in,
  // which is what `to_offset` means — and what makes it survive a round trip
  // through `<xdr:to>`, whose offset is measured into the cell after.
  const to = anchorPoint(rect.x + rect.w, rect.y + rect.h);
  let r1 = Math.max(from.row, to.row - 1);
  let c1 = Math.max(from.col, to.col - 1);
  let def;
  try {
    def = JSON.parse(wasm.session_chart_defs(state.sheet))[index];
  } catch { return; }
  if (!def) return;
  def.anchor = [from.row, from.col, r1, c1];
  def.fromOffset = [pxToEmu(from.dx), pxToEmu(from.dy)];
  // Degenerate frames get no trailing offset: it would be measured from a cell
  // edge the frame does not reach.
  const degenerate = to.row - 1 < from.row || to.col - 1 < from.col;
  def.toOffset = degenerate ? [0, 0] : [pxToEmu(to.dx), pxToEmu(to.dy)];
  panelChart = { sheet: state.sheet, index };
  applyChart(def);
}

/// The cell a pixel lands in and how far into it, which is exactly what a
/// drawing anchor stores.
function anchorPoint(px, py) {
  // Clamped once and used for both halves: taking the cell from a clamped
  // coordinate and the offset from an unclamped one puts the two out of step,
  // and an edge dragged past the frozen headers lands in the wrong cell by the
  // amount it overshot.
  const x = Math.max(HW, px), y = Math.max(HH, py);
  const row = rowAtY(y), col = colAtX(x);
  return { row, col, dx: x - fscreenX(col), dy: y - fscreenY(row) };
}

// Insert ▸ Chart: build one over the block under the cursor.
//
// The range is read the way Excel reads it — a text first row is headers, a
// text first column is labels — so the common case is one click. Everything the
// guess got wrong is in the panel, live.
function chartDialog(kind) {
  const r = effectiveRange();
  let bounds = r;
  if (r.r0 === r.r1 && r.c0 === r.c1) {
    try {
      const blk = JSON.parse(wasm.session_block_bounds(state.sheet, r.r0, r.c0));
      if (blk) bounds = { r0: blk.r0, c0: blk.c0, r1: blk.r1, c1: blk.c1 };
    } catch { /* fall through to the selection */ }
  }
  try {
    const index = wasm.session_create_chart(
      state.sheet, bounds.r0, bounds.c0, bounds.r1, bounds.c1, kind);
    panelChart = { sheet: state.sheet, index };
  } catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  openPanel("chart");
  status.textContent = "chart added — drag it to move, or use Move here";
}

// --- Pivot panel -----------------------------------------------------------
//
// Excel's PivotTable Fields pane, in one column: a field list you drag from,
// four areas you drag into, and the options underneath.
//
// The panel holds no state of its own. Every change rebuilds the whole
// definition from the DOM, sends it as one `session_set_pivot`, and redraws
// from what came back — so the panel cannot drift from the workbook, and each
// change is one undo step covering both the layout and the figures it produced.
//
// Which pivot is being edited is remembered rather than looked up from the
// cursor, because a pivot with nothing on its axes has written nothing yet and
// so covers no cell to find it by.
let panelPivot = null;

const PIVOT_AREAS = [
  ["filters", "Filters"],
  ["cols", "Columns"],
  ["rows", "Rows"],
  ["values", "Values"],
];

const PIVOT_AGGREGATES = [
  ["sum", "Sum"], ["count", "Count"], ["countNums", "Count numbers"],
  ["average", "Average"], ["max", "Max"], ["min", "Min"],
  ["product", "Product"], ["stdDev", "StdDev"], ["stdDevp", "StdDevp"],
  ["var", "Var"], ["varp", "Varp"],
];

// Sort cycles rather than opening a menu: three states, one button, and the
// glyph says which one it is in.
const PIVOT_SORTS = [
  ["ascending", "↑", "A→Z / smallest first"],
  ["descending", "↓", "Z→A / largest first"],
  ["dataSource", "⇅", "source order"],
];

// The name of the pivot whose report covers a cell, or "" — the guard the
// editor checks before letting anything be typed there.
function pivotBlocks(row, col) {
  try { return wasm.session_pivot_blocks(state.sheet, row, col); } catch { return ""; }
}

function pivotAt(row, col) {
  try {
    return JSON.parse(wasm.session_pivot_at(state.sheet, row, col));
  } catch { return null; }
}

function currentPivot() {
  if (!panelPivot) return null;
  try {
    const all = JSON.parse(wasm.session_pivots(panelPivot.sheet));
    return all[panelPivot.index] || null;
  } catch { return null; }
}

// Send the whole definition. `p` is the object `session_pivots` handed out,
// mutated in place by whichever control was touched.
function applyPivot(p) {
  try {
    wasm.session_set_pivot(panelPivot.sheet, panelPivot.index, JSON.stringify(p));
    status.textContent = p.values.length ? "pivot refreshed" : "add a field to Values";
  } catch (e) {
    statusError(errText(e));
  }
  invalidateGrowth();
  draw();
  refreshPivotPanel();
}

function refreshPivotPanel() {
  if (activePanel !== "pivot") return;
  const body = byId("side-panel-body");
  body.textContent = "";
  buildPivotPanel(body);
}

// Where a field currently sits, so the field list can grey out what is in use.
function pivotPlacement(p, field) {
  for (const [key] of PIVOT_AREAS) {
    if (p[key].some((f) => f.field === field)) return key;
  }
  return null;
}

function buildPivotPanel(body) {
  const p = currentPivot();
  if (!p) {
    body.appendChild(el("div", "panel-note",
      "Select a cell inside a pivot table, or Insert ▸ PivotTable."));
    return;
  }

  panelLabel(body, "Name");
  const name = el("input", "panel-field");
  name.type = "text";
  name.value = p.name;
  name.addEventListener("change", () => {
    const want = name.value.trim();
    if (!want || want === p.name) { name.value = p.name; return; }
    p.name = want;
    applyPivot(p);
  });
  name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.stopPropagation(); name.blur(); }
    else if (e.key === "Escape") { e.stopPropagation(); name.value = p.name; name.blur(); }
  });
  body.appendChild(name);

  const src = el("div", "panel-range",
    `${sheetNameAt(p.sourceSheet) || "?"}!${A1(p.source[0], p.source[1])}:${A1(p.source[2], p.source[3])}`);
  body.appendChild(src);
  if (p.imported) {
    body.appendChild(el("div", "panel-note",
      `From the file. Refreshing rewrites it in ${BRAND}'s layout and replaces `
      + "the definition Excel saved."));
  }

  // --- the field list, and the four areas ---------------------------------
  panelLabel(body, "Fields");
  const list = el("div", "pivot-fields");
  p.fields.forEach((label, index) => {
    const chip = el("div", "pivot-chip pivot-chip-src", label || `Column${index + 1}`);
    chip.draggable = true;
    const where = pivotPlacement(p, index);
    if (where) chip.classList.add("in-use");
    chip.title = where ? `in ${where}` : "drag into an area below";
    chip.addEventListener("dragstart", (e) => {
      e.dataTransfer.setData("text/plain", JSON.stringify({ from: "fields", field: index }));
      e.dataTransfer.effectAllowed = "copy";
    });
    // Double-click is the keyboard-free shortcut Excel has too: it drops the
    // field where its type suggests — numbers summarize, text groups.
    chip.addEventListener("dblclick", () => {
      if (where) return;
      const target = pivotFieldIsNumeric(p, index) ? "values" : "rows";
      pivotAdd(p, target, index, p[target].length);
    });
    list.appendChild(chip);
  });
  body.appendChild(list);

  for (const [key, title] of PIVOT_AREAS) {
    panelLabel(body, title);
    const zone = el("div", "pivot-zone");
    zone.dataset.area = key;
    if (!p[key].length) zone.appendChild(el("div", "pivot-zone-empty", "drag a field here"));
    p[key].forEach((f, i) => zone.appendChild(pivotChip(p, key, f, i)));
    zone.addEventListener("dragover", (e) => {
      e.preventDefault();
      zone.classList.add("over");
    });
    zone.addEventListener("dragleave", () => zone.classList.remove("over"));
    zone.addEventListener("drop", (e) => {
      e.preventDefault();
      zone.classList.remove("over");
      let payload;
      try { payload = JSON.parse(e.dataTransfer.getData("text/plain")); } catch { return; }
      pivotDrop(p, key, payload, pivotDropIndex(zone, e.clientY));
    });
    body.appendChild(zone);
  }

  // --- options -------------------------------------------------------------
  panelLabel(body, "Totals");
  for (const [key, label] of [["rowGrandTotals", "Grand total row"],
                              ["colGrandTotals", "Grand total column"]]) {
    const row = el("label", "panel-check");
    const box = el("input");
    box.type = "checkbox";
    box.checked = !!p[key];
    box.addEventListener("change", () => { p[key] = box.checked; applyPivot(p); });
    row.appendChild(box);
    row.appendChild(el("span", null, label));
    body.appendChild(row);
  }

  panelLabel(body, "Style");
  const style = el("select", "panel-select");
  for (const [label, value] of TABLE_STYLES) {
    const o = el("option", null, label);
    o.value = value;
    style.appendChild(o);
  }
  style.value = p.style || "TableStyleMedium2";
  style.addEventListener("change", () => { p.style = style.value; applyPivot(p); });
  body.appendChild(style);

  panelActions(body, "Refresh", () => {
    try {
      wasm.session_refresh_pivot(panelPivot.sheet, panelPivot.index);
      status.textContent = "pivot refreshed";
    } catch (e) { statusError(errText(e)); }
    invalidateGrowth();
    draw();
    refreshPivotPanel();
  }, "Delete", async () => {
    if (!await confirmModal("Delete pivot table",
      `Delete “${p.name}” and the report it wrote?`, "Delete")) return;
    tryEdit(() => wasm.session_delete_pivot(panelPivot.sheet, panelPivot.index));
    panelPivot = null;
    refreshPivotPanel();
  });
}

// Whether a field's data reads as numbers, which is what decides whether a
// double-click summarizes it or groups by it — Excel's own rule.
function pivotFieldIsNumeric(p, field) {
  let items = [];
  try {
    items = JSON.parse(wasm.session_pivot_items(panelPivot.sheet, panelPivot.index, field));
  } catch { return false; }
  if (!items.length) return false;
  return items.every((v) => v === "(blank)" || (v.trim() !== "" && !Number.isNaN(Number(v))));
}

// One placed field: its name, the controls its area gives it, and a remove
// button.
function pivotChip(p, area, f, index) {
  const chip = el("div", "pivot-chip pivot-chip-set");
  chip.draggable = true;
  chip.dataset.index = String(index);
  chip.addEventListener("dragstart", (e) => {
    e.dataTransfer.setData("text/plain", JSON.stringify({ from: area, field: f.field, index }));
    e.dataTransfer.effectAllowed = "move";
  });
  chip.appendChild(el("span", "pivot-chip-name", p.fields[f.field] || `Column${f.field + 1}`));

  if (area === "rows" || area === "cols") {
    const cur = PIVOT_SORTS.findIndex(([v]) => v === (f.sort || "ascending"));
    const [, glyph, hint] = PIVOT_SORTS[cur < 0 ? 0 : cur];
    const sort = el("button", "pivot-mini", glyph);
    sort.title = `Sort: ${hint}`;
    sort.addEventListener("click", () => {
      f.sort = PIVOT_SORTS[((cur < 0 ? 0 : cur) + 1) % PIVOT_SORTS.length][0];
      applyPivot(p);
    });
    chip.appendChild(sort);
    // The innermost field's subtotal would restate the line above it, so it is
    // never emitted — and a switch that does nothing is worse than no switch.
    if (index < p[area].length - 1) {
      const sub = el("button", "pivot-mini" + (f.subtotal ? " on" : ""), "Σ");
      sub.title = f.subtotal ? "Subtotals on" : "Subtotals off";
      sub.addEventListener("click", () => { f.subtotal = !f.subtotal; applyPivot(p); });
      chip.appendChild(sub);
    }
  } else if (area === "values") {
    const agg = el("select", "pivot-agg");
    for (const [value, label] of PIVOT_AGGREGATES) {
      const o = el("option", null, label);
      o.value = value;
      agg.appendChild(o);
    }
    agg.value = f.aggregate || "sum";
    agg.addEventListener("change", () => { f.aggregate = agg.value; applyPivot(p); });
    chip.appendChild(agg);
  } else {
    const shown = !f.selected.length ? "(All)"
      : f.selected.length === 1 ? f.selected[0]
        : `(${f.selected.length} items)`;
    const pick = el("button", "pivot-mini pivot-mini-wide", shown + " ▾");
    pick.title = "Choose which values to include";
    pick.addEventListener("click", () => pivotItemPicker(p, f, chip));
    chip.appendChild(pick);
  }

  const remove = el("button", "pivot-mini pivot-remove", "✕");
  remove.title = "Remove from " + area;
  remove.addEventListener("click", () => { p[area].splice(index, 1); applyPivot(p); });
  chip.appendChild(remove);
  return chip;
}

// The page filter's checklist, inline under its chip rather than in a popup:
// the panel is already a narrow column, and a floating list over it would cover
// the thing being filtered.
function pivotItemPicker(p, f, chip) {
  const open = chip.parentElement.querySelector(".pivot-items");
  if (open) { open.remove(); return; }
  let items = [];
  try {
    items = JSON.parse(wasm.session_pivot_items(panelPivot.sheet, panelPivot.index, f.field));
  } catch { /* an unreadable source lists nothing */ }
  const box = el("div", "pivot-items");
  const chosen = new Set(f.selected);

  const all = el("label", "panel-check");
  const allBox = el("input");
  allBox.type = "checkbox";
  // Empty means every value, which is the `(All)` state — not "none selected".
  allBox.checked = chosen.size === 0;
  allBox.addEventListener("change", () => { f.selected = []; applyPivot(p); });
  all.appendChild(allBox);
  all.appendChild(el("span", null, "(All)"));
  box.appendChild(all);

  for (const item of items) {
    const row = el("label", "panel-check");
    const cb = el("input");
    cb.type = "checkbox";
    cb.checked = chosen.size === 0 || chosen.has(item);
    cb.addEventListener("change", () => {
      const next = chosen.size === 0 ? new Set(items) : new Set(chosen);
      if (cb.checked) next.add(item); else next.delete(item);
      // Everything ticked is the same as nothing chosen, and storing it as
      // `(All)` keeps the pivot following values added to the source later.
      f.selected = next.size === items.length ? [] : [...next];
      applyPivot(p);
    });
    row.appendChild(cb);
    row.appendChild(el("span", null, item));
    box.appendChild(row);
  }
  chip.after(box);
}

// Which slot a drop lands in: above the first chip whose midpoint is below the
// pointer. Order is the nesting order, so this is not cosmetic — dropping
// Region above Product is a different report from the other way round.
function pivotDropIndex(zone, clientY) {
  const chips = [...zone.querySelectorAll(".pivot-chip-set")];
  for (let i = 0; i < chips.length; i++) {
    const box = chips[i].getBoundingClientRect();
    if (clientY < box.top + box.height / 2) return i;
  }
  return chips.length;
}

function pivotDrop(p, area, payload, at) {
  if (payload.from === "fields") {
    if (pivotPlacement(p, payload.field)) {
      status.textContent = "that field is already in use — drag it out first";
      return;
    }
    pivotAdd(p, area, payload.field, at);
    return;
  }
  // Moving between (or within) areas: take it out first, then put it back, so
  // a reorder inside one area lands where the pointer says rather than one slot
  // late.
  const [moved] = p[payload.from].splice(payload.index, 1);
  if (!moved) return;
  const index = payload.from === area && payload.index < at ? at - 1 : at;
  pivotInsert(p, area, moved, index);
  applyPivot(p);
}

function pivotAdd(p, area, field, at) {
  pivotInsert(p, area, { field }, at);
  applyPivot(p);
}

// A field carries what its new area needs and drops what it does not: an
// aggregate is meaningless on the row axis, and a sort order is meaningless on
// a measure.
function pivotInsert(p, area, f, at) {
  const entry = area === "values"
    ? { field: f.field, aggregate: f.aggregate || "sum", name: "", numberFormat: f.numberFormat || null }
    : area === "filters"
      ? { field: f.field, selected: f.selected || [] }
      : { field: f.field, sort: f.sort || "ascending", subtotal: f.subtotal !== false };
  p[area].splice(Math.max(0, Math.min(at, p[area].length)), 0, entry);
}

// Insert ▸ PivotTable: build one over the block under the cursor, on a new
// sheet.
//
// A new sheet is Excel's own default and the only destination that cannot
// collide with something already on the grid. Nothing else is asked: every
// choice a dialog would make is in the panel, live, and one undo step away.
async function pivotDialog() {
  const here = pivotAt(state.sel.row, state.sel.col);
  if (here) { panelPivot = { sheet: state.sheet, index: here.index }; openPanel("pivot"); return; }

  const r = effectiveRange();
  let bounds = r;
  if (r.r0 === r.r1 && r.c0 === r.c1) {
    try {
      const blk = JSON.parse(wasm.session_block_bounds(state.sheet, r.r0, r.c0));
      if (blk) bounds = { r0: blk.r0, c0: blk.c0, r1: blk.r1, c1: blk.c1 };
    } catch { /* fall through to the selection */ }
  }
  if (bounds.r1 <= bounds.r0) {
    statusError("select a table with a header row and at least one row of data");
    return;
  }
  const source = state.sheet;
  let dest;
  try {
    dest = wasm.session_add_sheet();
    const index = wasm.session_create_pivot(
      source, bounds.r0, bounds.c0, bounds.r1, bounds.c1, dest, 0, 0, "");
    panelPivot = { sheet: dest, index };
  } catch (e) { statusError(errText(e)); return; }
  renderTabs();
  switchSheet(dest);
  invalidateGrowth();
  draw();
  openPanel("pivot");
  status.textContent = "drag a field into Values to see the report";
}

// --- Localization -------------------------------------------------------------
//
// Two injection points, because they serve two different people: a host that
// already knows its user's language supplies one, and a user who wants to
// choose picks from the footer.
//
// Messages are keyed by **command id**, which is derived from the *English*
// label. That is deliberate and load-bearing: translating the labels must not
// renumber the command API, so `commandId()` always sees English and only the
// rendered text changes. Getting this backwards would mean `format.bold`
// becoming `format.fett` for a German host, and every `commands({hidden})` list
// silently ceasing to match.
//
// What is covered today: menu items, submenu headings and toolbar tooltips —
// the labels a user reads constantly. Panels and dialogs are still English
// until their catalogues are written; a missing key falls back to the English
// string rather than showing a key, so partial coverage degrades to "some of it
// is translated" rather than to gibberish.

let locale = "en-US";
const messages = new Map();

/// Look a message up, falling back to the English text it was keyed from.
function t(key, fallback) {
  return messages.get(locale)?.[key] ?? fallback;
}

/// Install a catalogue. Merges, so a host can override three strings without
/// restating the language.
export function setMessages(forLocale, map) {
  messages.set(forLocale, { ...(messages.get(forLocale) ?? {}), ...map });
  relabel();
}

export function setLocale(next) {
  locale = next || "en-US";
  syncLocalePicker();
  relabel();
}

/// Show or hide the footer language control, and fill it.
export function setLocalePicker(on) {
  const box = byId("locale-picker");
  if (box) box.hidden = !on;
  syncLocalePicker();
}

function syncLocalePicker() {
  const select = byId("locale-select");
  if (!select) return;
  const locales = availableLocales();
  if (select.options.length !== locales.length) {
    select.textContent = "";
    for (const code of locales) {
      const option = document.createElement("option");
      option.value = code;
      // The language's own name where the platform knows it, because a picker
      // that lists "German" to a German speaker is a picker for someone else.
      let label = code;
      try {
        label = new Intl.DisplayNames([code], { type: "language" }).of(code.split("-")[0]) ?? code;
      } catch { /* an unknown tag keeps its code, which is still choosable */ }
      option.textContent = label;
      select.append(option);
    }
    select.onchange = () => setLocale(select.value);
  }
  select.value = locale;
}

export function getLocale() {
  return locale;
}

/// The locales a catalogue has been supplied for, plus the built-in one.
export function availableLocales() {
  return ["en-US", ...[...messages.keys()].filter((l) => l !== "en-US")].sort();
}

/// Which menu each Alt mnemonic opens. Rebuilt whenever the labels change,
/// because a translated menu bar has different letters free.
const menuMnemonics = new Map();

/// Label the top-level menu bar and assign its Alt mnemonics.
///
/// Not always the first letter: File and Format both start with F, so the
/// naive rule left Format unreachable *and* advertising a shortcut belonging to
/// File. Take the first character not already claimed — which is how Windows
/// menus have always assigned these, and which has to be recomputed per
/// language rather than baked in at build time.
function relabelMenubar() {
  menuMnemonics.clear();
  for (const btn of qsa(".menubar .menu-top")) {
    const english = btn.dataset.ocLabel ?? btn.textContent;
    const name = t(`command.${btn.dataset.ocCommand}`, english);
    const index = Number(btn.dataset.ocMenuIndex ?? -1);
    let at = [...name].findIndex((ch) => !menuMnemonics.has(ch.toLowerCase()));
    if (at < 0) at = 0; // every letter taken: no mnemonic, but still labelled
    const key = name[at].toLowerCase();
    if (!menuMnemonics.has(key) && index >= 0) menuMnemonics.set(key, index);
    // The letter is wrapped so it can be underlined without changing layout.
    // Built as nodes rather than markup: a translated label is host-supplied
    // text and must not be able to inject elements.
    btn.textContent = "";
    btn.append(name.slice(0, at));
    const mn = document.createElement("span");
    mn.className = "mn";
    mn.textContent = name[at];
    btn.append(mn, name.slice(at + 1));
    btn.setAttribute("aria-keyshortcuts", `Alt+${name[at].toUpperCase()}`);
  }
}

/// Re-render every label that came from a catalogue.
///
/// Cheaper and far less error-prone than rebuilding the menus: each labelled
/// node remembers its English source in a data attribute, so relabelling is a
/// pass over the DOM rather than a teardown.
function relabel() {
  relabelMenubar();
  // The roster's own strings come from the catalogue too, and it is built in
  // JS rather than carried in the markup, so a language change has to rebuild
  // it. A no-op outside a session.
  renderPresence();
  for (const node of qsa("[data-oc-label]")) {
    if (node.classList.contains("menu-top")) continue; // handled above
    const id = node.dataset.ocCommand;
    const english = node.dataset.ocLabel;
    const text = id ? t(`command.${id}`, english) : english;
    const slot = node.querySelector(".mi-label");
    if (slot) slot.textContent = text;
    else node.textContent = text;
  }
  for (const node of qsa("[data-oc-tip]")) {
    const text = t(`tip.${node.dataset.ocCommand ?? node.id}`, node.dataset.ocTip);
    // Write it back where the tooltip is actually read from. A tipified node
    // has no `title` any more — setting one would translate nothing and
    // resurrect the native bubble beside our own.
    if (node.dataset.tip !== undefined) {
      node.dataset.tip = text;
      node.setAttribute("aria-label", text);
    } else {
      node.title = text;
    }
  }
  updateCellMode();
}

// --- Events ------------------------------------------------------------------
//
// `before*` / past-tense pairs, `before*` cancellable, every event carrying a
// `source`. That is Handsontable's design and it is right for two reasons a
// host feels immediately: without cancellation it cannot enforce its own
// permissions, and without a `source` it cannot tell its own programmatic
// write from a user's keystroke — so it echoes its own change back into its
// store and loops.
//
// Granularity is **one event per operation**, carrying a range. A paste of a
// hundred thousand cells is one `cellsChanged`, not a hundred thousand of them,
// which matches how the transaction layer already batches and is the only
// version that stays usable at size.

const listeners = new Map();

/// Subscribe. Returns an unsubscribe function, so a caller need not keep the
/// handler around to remove it later.
export function on(name, handler) {
  if (!listeners.has(name)) listeners.set(name, new Set());
  listeners.get(name).add(handler);
  return () => off(name, handler);
}

export function off(name, handler) {
  listeners.get(name)?.delete(handler);
}

/// Emit, returning false if a `before*` handler cancelled.
///
/// A throwing handler must not take the editor down with it: a host's bug in a
/// change listener would otherwise make the grid unusable, which is a far worse
/// failure than the one they wrote.
function emit(name, detail) {
  const set = listeners.get(name);
  if (!set || !set.size) return true;
  let prevented = false;
  const event = { ...detail, preventDefault: () => { prevented = true; } };
  for (const handler of [...set]) {
    try {
      if (handler(event) === false) prevented = true;
    } catch (err) {
      console.error(`[opencalc] ${name} listener threw`, err);
    }
  }
  return !prevented;
}

/// The last state reported, so a change is only announced when it changed.
const lastReported = { selection: "", dirty: null, calc: "", undo: "" };

/// Emit whatever has changed since the previous frame.
///
/// Polled from `draw()` rather than fired at each mutation site: there are
/// dozens of those and one of them will always be forgotten, whereas the paint
/// is the one place everything already funnels through.
function emitStateEvents() {
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

// --- Commands ---------------------------------------------------------------
//
// Every menu item and toolbar button carries a stable id, so a host can hide or
// disable *individual* controls rather than whole regions — and so read-only
// can take editing off the menus rather than only refusing the keystroke.
//
// Ids are derived from the English label path (`Format ▸ Alignment ▸ Left` →
// `format.alignment.left`) rather than hand-assigned. That keeps them
// predictable and impossible to forget on a new item; the cost is that
// renaming a label renames its id, which the docs state plainly and which we
// treat as a breaking change like any other API rename.

function commandId(path, label) {
  const slug = String(label)
    .replace(/[…\u2026]/g, "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  return path ? `${path}.${slug}` : slug;
}

/// Commands a viewer may still run.
///
/// A **whitelist**, deliberately. With a blacklist, an editing command added
/// later and not added to the list leaks into read-only mode; with a whitelist
/// the worst case is that something harmless is hidden until someone notices.
/// Getting that backwards is the difference between a cosmetic bug and a
/// workbook a viewer could change.
const READ_ONLY_SAFE = [
  // Reading the sheet, in every sense.
  /^edit\.copy$/, /^edit\.select-all$/,
  // No toolbar entry: every control on it applies formatting, the format
  // painter included — it reads a style from one cell and *writes* it to
  // another, which a first pass wrongly let through.
  // Zoom is the editor's own state, not the workbook's — a viewer must be able
  // to zoom.
  /^view\.zoom/,
  // Recalculating is not an edit: it produces the values the formulas already
  // imply, and a viewer that cannot compute is showing stale numbers.
  /^tools\.calculation/,
  // Getting a copy out, and putting it on paper.
  /^file\.download/, /^file\.print$/,
  /^help\./,
];

// Everything else in `view.` is deliberately absent. Freezing panes, hiding
// gridlines, showing formulas and showing zeros all live in the *workbook* —
// they travel in the file and go through `SetSheetMetadata`, which a read-only
// session refuses. Offering them would be offering something that then fails.

const isReadOnlySafe = (id) => READ_ONLY_SAFE.some((re) => re.test(id));

/// Host-supplied `{ hidden: [], disabled: [] }`.
let commandRules = { hidden: [], disabled: [] };

/// Every command id present in this mount, for a host that wants to discover
/// them rather than read a list in the docs that can go stale.
export function listCommands() {
  return [...qsa("[data-oc-command]")].map((n) => n.dataset.ocCommand).filter((v, i, a) => a.indexOf(v) === i).sort();
}

/// Hide or disable commands by id.
export function setCommandRules(rules) {
  commandRules = {
    hidden: rules?.hidden ?? [],
    disabled: rules?.disabled ?? [],
  };
  applyCommandRules();
}

/// Apply the host's rules *and* read-only, which hides every editing command.
///
/// Hidden rather than disabled for read-only: a viewer is not a broken editor,
/// and a menu of greyed-out things a user can never enable is worse than a
/// short menu. A host that would rather grey them can say so per command.
export function applyCommandRules() {
  const viewer = readOnly();
  const hide = (node, on) => {
    node.hidden = on;
    node.classList.toggle("oc-cmd-hidden", on);
  };

  for (const node of qsa("[data-oc-command]")) {
    // A top-level menu button is not a command in its own right — `file` is a
    // heading, not something anyone can run. Deciding it by the whitelist hid
    // every menu in read-only and left an empty bar. It is decided below, by
    // whether anything inside it survived.
    if (node.classList.contains("menu-top")) continue;
    const id = node.dataset.ocCommand;
    const off = commandRules.hidden.includes(id) || (viewer && !isReadOnlySafe(id));
    hide(node, off);
    const dim = !off && commandRules.disabled.includes(id);
    node.disabled = dim;
    node.classList.toggle("oc-cmd-disabled", dim);
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
  let anyGroup = false;
  for (const group of qsa(".tb-group")) {
    const live = [...group.querySelectorAll("[data-oc-command]")].some((n) => !n.hidden);
    anyGroup ||= live;
    group.hidden = !live;
    group.classList.toggle("oc-cmd-hidden", !live);
  }
  // Every control on the toolbar formats something, so in a viewer the whole
  // strip is empty. Removing it is not a policy choice the host should have to
  // make — it is the honest consequence of there being nothing in it.
  const toolbar = qs(".toolbar");
  if (toolbar) toolbar.classList.toggle("oc-cmd-hidden", !anyGroup);
}

// --- Calculation mode ------------------------------------------------------
//
// Excel's Formulas ▸ Calculation Options. A workbook saved with calculation
// turned off opens that way, so this is not a preference the editor invents —
// it is state the file carries and the user has to be able to see and change.

function calcMode() {
  try { return wasm.session_calculation_mode(); } catch { return "auto"; }
}

function setCalculationMode(mode) {
  try { wasm.session_set_calculation_mode(mode); } catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  status.textContent = mode === "manual"
    ? "manual calculation — press F9 to calculate"
    : "automatic calculation";
}

// --- Stopping a long job (`SEC-017`) ---------------------------------------
//
// The engine can be stopped mid-job; until now nothing here asked it to, so a
// workbook that was inside every admission bound and simply enormous held the
// tab's one thread until it finished.
//
// **The limit is a number set up front, not a Stop button, and that is forced
// rather than chosen.** JavaScript and WebAssembly share the one thread a tab
// has: while the engine runs, no click handler runs, so a Stop button's
// listener could not fire until the job it was meant to stop had already
// returned. What the editor can do is bound the job before starting it and
// hand the choice back afterwards — which is what "Keep waiting" is.

/// How long an open may hold the thread before it is stopped.
let openBudgetMs = 10_000;
/// The same for a full recalculation (F9), which is the other long job.
let recalcBudgetMs = 5_000;

/// Set both limits, for the browser gate.
///
/// Exported because the only honest test of a cancellation is one that provokes
/// it, and provoking a ten-second limit for real would mean a ten-second test
/// and a workbook big enough to need one. A negative value means no limit.
export function setTimeBudgetsForTest(openMs, recalcMs) {
  openBudgetMs = openMs;
  recalcBudgetMs = recalcMs;
}

/// Whether an engine error is "this job was stopped" rather than "this file is
/// bad".
///
/// Matched on the stable diagnostic code (docs/20) rather than the prose: the
/// sentence is allowed to be reworded, the code is not.
function wasStopped(err) {
  return /OC-IMP-0007/.test(String(err && err.message ? err.message : err));
}

/// Take away any outstanding "Keep waiting" offer.
function clearKeepWaiting() {
  byId("keep-waiting")?.remove();
}

/// Offer to re-run `again` with no limit at all.
///
/// A limit the user cannot overrule is not a limit, it is a refusal: a workbook
/// that genuinely takes twenty seconds is still that user's workbook, and this
/// is the whole of the "cancel" affordance a single-threaded host can honestly
/// offer — the choice comes *after* the stop, because nothing the user does can
/// arrive during one.
///
/// The button is a sibling of the status text rather than a child of it: the
/// status bar is one ellipsised line with a `max-width`, so a control inside it
/// is a control clipped out of reach.
function offerKeepWaiting(what, again) {
  const bar = byId("tb-status");
  if (!bar || !bar.parentNode) return;
  clearKeepWaiting();
  const note = document.createElement("span");
  note.className = "warn";
  note.textContent = ` — stopped ${what} before it finished`;
  bar.append(note);
  const btn = document.createElement("button");
  btn.id = "keep-waiting";
  btn.className = "oc-btn keep-waiting";
  btn.textContent = "Keep waiting";
  btn.addEventListener("click", () => { btn.remove(); again(); });
  bar.parentNode.insertBefore(btn, bar.nextSibling);
}

// F9: recompute everything, and reseed first so RAND rerolls — Excel does the
// same. Not an undoable edit and it does not dirty the document: the values it
// produces are the ones the formulas already imply.
//
// `budgetMs` is how long it may run; a negative one means no limit, which is
// what "Keep waiting" retries with.
function recalculateNow(budgetMs = undefined) {
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

/// Whether the session refuses edits.
function readOnly() {
  try { return wasm.session_read_only(); } catch { return false; }
}

/// Open the workbook for reading only, or release it.
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

// Whether an edit is waiting on a manual calculation, for the status bar —
// Excel writes "Calculate" there and it is the only cue that what is on screen
// is not what the formulas say.
function needsRecalc() {
  try { return wasm.session_needs_recalculation(); } catch { return false; }
}

// Alt+F5 — recompute the pivot under the cursor from its source.
function refreshPivotHere() {
  const here = pivotAt(state.sel.row, state.sel.col);
  if (!here) { status.textContent = "no pivot table here"; return; }
  try {
    wasm.session_refresh_pivot(state.sheet, here.index);
    status.textContent = `refreshed ${here.name}`;
  } catch (e) { statusError(errText(e)); }
  invalidateGrowth();
  draw();
  refreshPivotPanel();
}

// Ctrl+Alt+F5 — every pivot in the workbook.
//
// One refusal does not fail the command: the others are still worth
// recomputing, and the ones that could not are named rather than counted.
function refreshAllPivots() {
  let problems = "";
  try { problems = wasm.session_refresh_all_pivots(); }
  catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  refreshPivotPanel();
  const failed = problems.split("\n").filter(Boolean);
  if (failed.length) statusError(`could not refresh: ${failed.join(", ")}`);
  else status.textContent = "pivots refreshed";
}

// Page setup: everything OOXML records about printing a sheet, all of which was
// being carried through every save with nothing able to change it.
//
// Applied on change, one `session_set_page_setup` call each, so every switch is
// its own undo step and the panel never holds state the workbook does not.
function buildPagePanel(body) {
  const get = () => { try { return JSON.parse(wasm.session_page_setup(state.sheet)); } catch { return {}; } };
  let cur = get();
  const set = (pairs) => {
    tryEdit(() => wasm.session_set_page_setup(
      state.sheet, Object.keys(pairs), Object.values(pairs).map((v) => String(v))));
    cur = get();
  };

  panelLabel(body, "Orientation");
  const orient = el("select", "panel-select");
  for (const [v, t] of [["portrait", "Portrait"], ["landscape", "Landscape"]]) {
    const o = el("option", null, t); o.value = v; orient.appendChild(o);
  }
  orient.value = cur["page.orientation"] || "portrait";
  orient.addEventListener("change", () => set({ "page.orientation": orient.value }));
  body.appendChild(orient);

  panelLabel(body, "Paper");
  const paper = el("select", "panel-select");
  // `paperSize` is a numbered enum; these are the sizes people actually pick.
  for (const [v, t] of [["1", "Letter"], ["5", "Legal"], ["8", "A3"], ["9", "A4"], ["11", "A5"]]) {
    const o = el("option", null, t); o.value = v; paper.appendChild(o);
  }
  paper.value = cur["page.paperSize"] || "1";
  paper.addEventListener("change", () => set({ "page.paperSize": paper.value }));
  body.appendChild(paper);

  panelLabel(body, "Scale");
  const scaleWrap = el("div", "oc-totals-grid");
  const scale = el("input", "panel-field");
  scale.type = "number"; scale.min = "10"; scale.max = "400";
  scale.value = cur["page.scale"] || "100";
  scale.addEventListener("change", () => set({
    "page.scale": scale.value,
    // Scaling and fit-to-page are alternatives in Excel; setting one has to
    // clear the other or the file asks for both and the reader picks.
    "setupPr.fitToPage": "",
  }));
  scaleWrap.append(el("span", "oc-totals-col", "Percent"), scale);
  const fitW = el("input", "panel-field");
  fitW.type = "number"; fitW.min = "0";
  fitW.value = cur["page.fitToWidth"] || "1";
  const fitH = el("input", "panel-field");
  fitH.type = "number"; fitH.min = "0";
  fitH.value = cur["page.fitToHeight"] || "1";
  const fitOn = () => set({
    "setupPr.fitToPage": "1",
    "page.fitToWidth": fitW.value,
    "page.fitToHeight": fitH.value,
    "page.scale": "",
  });
  fitW.addEventListener("change", fitOn);
  fitH.addEventListener("change", fitOn);
  scaleWrap.append(el("span", "oc-totals-col", "Fit to width"), fitW);
  scaleWrap.append(el("span", "oc-totals-col", "Fit to height"), fitH);
  body.appendChild(scaleWrap);

  panelLabel(body, "Margins (inches)");
  const mg = el("div", "oc-totals-grid");
  for (const [key, label, dflt] of [
    ["top", "Top", "0.75"], ["bottom", "Bottom", "0.75"],
    ["left", "Left", "0.7"], ["right", "Right", "0.7"],
  ]) {
    const i = el("input", "panel-field");
    i.type = "number"; i.step = "0.05"; i.min = "0";
    i.value = cur["margins." + key] || dflt;
    i.addEventListener("change", () => set({ ["margins." + key]: i.value }));
    mg.append(el("span", "oc-totals-col", label), i);
  }
  body.appendChild(mg);

  panelLabel(body, "Print");
  const checks = el("div", "oc-table-checks");
  const check = (label, key, group) => {
    const l = el("label", "oc-check");
    const i = document.createElement("input");
    i.type = "checkbox";
    i.checked = cur[group + "." + key] === "1" || cur[group + "." + key] === "true";
    i.addEventListener("change", () => set({ [group + "." + key]: i.checked ? "1" : "" }));
    l.append(i, document.createTextNode(" " + label));
    checks.appendChild(l);
  };
  check("Gridlines", "gridLines", "options");
  check("Row/column headings", "headings", "options");
  check("Centre across", "horizontalCentered", "options");
  check("Centre down", "verticalCentered", "options");
  body.appendChild(checks);

  panelLabel(body, "What prints");
  let scope = {};
  try { scope = JSON.parse(wasm.session_print_scope(state.sheet)); } catch {}
  const scopeRow = (label, current, onSet, onClear) => {
    body.appendChild(el("div", "panel-range", current || "(all of it)"));
    const row = el("div", "panel-actions");
    const clear = el("button", "panel-btn-ghost", "Clear");
    clear.addEventListener("click", () => { onClear(); openPanel("page"); });
    const set = el("button", "panel-btn-ghost", label);
    set.addEventListener("click", () => { onSet(); openPanel("page"); });
    row.append(clear, set);
    body.appendChild(row);
  };
  panelLabel(body, "Print area");
  scopeRow(
    "Set from selection",
    scope.area,
    () => { const r = effectiveRange();
      tryEdit(() => wasm.session_set_print_area(state.sheet, r.r0, r.c0, r.r1, r.c1)); },
    () => tryEdit(() => wasm.session_clear_print_area(state.sheet)),
  );
  panelLabel(body, "Repeat rows at the top");
  scopeRow(
    "Set from selection",
    scope.titles,
    () => { const r = effectiveRange();
      tryEdit(() => wasm.session_set_print_title_rows(state.sheet, r.r0, r.r1)); },
    // r1 < r0 clears it — the engine's own signal for "no titles".
    () => tryEdit(() => wasm.session_set_print_title_rows(state.sheet, 1, 0)),
  );

  panelLabel(body, "Header and footer");
  for (const [key, ph] of [["oddHeader", "Header"], ["oddFooter", "Footer"]]) {
    const i = el("input", "panel-field");
    i.placeholder = ph + " — &L left, &C centre, &R right, &P page";
    i.value = cur["hf." + key] || "";
    i.addEventListener("change", () => set({ ["hf." + key]: i.value }));
    body.appendChild(i);
  }

  panelActions(body, "Print…", () => printSheet(), "Close", () => closePanel());
}

// Open the sheet as a printable page and hand it to the browser's print dialog.
//
// A separate window rather than a print stylesheet over the app: the grid is a
// canvas, so there is nothing for a stylesheet to lay out across pages.
function printSheet() {
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

function openPanel(tool) {
  const panel = byId("side-panel");
  activePanel = tool;
  panelRangeEls = [];
  panelNote = null;
  byId("side-panel-title").textContent =
    tool === "dv" ? "Data validation"
      : tool === "cf" ? "Conditional formatting"
      : tool === "table" ? "Table"
      : tool === "pivot" ? "PivotTable fields"
      : tool === "chart" ? "Chart"
      : tool === "page" ? "Page setup"
      : "Comments";
  const body = byId("side-panel-body");
  body.textContent = "";
  if (tool === "dv") buildDvPanel(body);
  else if (tool === "cf") buildCfPanel(body);
  else if (tool === "table") buildTablePanel(body);
  else if (tool === "pivot") buildPivotPanel(body);
  else if (tool === "chart") buildChartPanel(body);
  else if (tool === "page") buildPagePanel(body);
  else buildNotePanel(body);
  panel.hidden = false;
  resize(); // the grid narrows — refit the canvas to its new width
}

function closePanel() {
  const panel = byId("side-panel");
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
  } else if (activePanel === "table") {
    // Moving into a different table has to re-target the panel, or renaming
    // would rename whichever table it was opened on.
    const t = currentTable();
    const shown = byId("side-panel-body").dataset.table;
    const key = t ? `${t.name}@${t.r0},${t.c0}` : "";
    if (key !== shown) {
      byId("side-panel-body").dataset.table = key;
      refreshTablePanel();
    }
  } else if (activePanel === "pivot") {
    // Clicking into a different pivot's report re-targets the panel. Clicking
    // outside every report does not: a pivot with nothing on its axes has
    // written no cells to be found by, and dropping the panel the moment the
    // cursor moved would make it unusable exactly when it is being set up.
    const here = pivotAt(state.sel.row, state.sel.col);
    if (here && (!panelPivot || panelPivot.sheet !== state.sheet || panelPivot.index !== here.index)) {
      panelPivot = { sheet: state.sheet, index: here.index };
      refreshPivotPanel();
    }
  } else if (activePanel === "note" && panelNote) {
    const addr = A1(state.sel.row, state.sel.col);
    if (addr !== panelNote.cell) {
      panelNote.cell = addr;
      // A half-typed reply belongs to the cell it was started on, so moving
      // away clears it rather than carrying it to someone else's thread.
      panelNote.ta.value = "";
      panelNote.refresh();
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
  // `stop` is the only style that actually refuses the entry; the other two
  // let the value through. Carrying the attribute without offering it turned
  // every advisory rule in an opened file into a hard block.
  const styleSel = el("select", "panel-select");
  for (const [v, t] of [
    ["stop", "Stop — refuse the value"],
    ["warning", "Warning — allow, but say so"],
    ["information", "Information — allow, with a note"],
  ]) { const o = el("option", null, t); o.value = v; styleSel.appendChild(o); }
  const errTitle = el("input", "panel-field");
  errTitle.placeholder = "Title (optional)";
  errTitle.spellcheck = false;
  const msg = el("input", "panel-field");
  msg.placeholder = "Optional message";
  msg.spellcheck = false;
  const blankWrap = el("label", "panel-check");
  const blank = document.createElement("input");
  blank.type = "checkbox";
  blank.checked = true;
  blankWrap.append(blank, document.createTextNode(" allow an empty cell"));
  const hideWrap = el("label", "panel-check");
  const hideDrop = document.createElement("input");
  hideDrop.type = "checkbox";
  hideWrap.append(hideDrop, document.createTextNode(" no in-cell dropdown"));
  body.append(styleSel, errTitle, msg, blankWrap, hideWrap);

  panelLabel(body, "Hint shown when the cell is selected");
  const promptTitle = el("input", "panel-field");
  promptTitle.placeholder = "Title (optional)";
  promptTitle.spellcheck = false;
  const promptText = el("input", "panel-field");
  promptText.placeholder = "e.g. Pick a region from the list";
  promptText.spellcheck = false;
  body.append(promptTitle, promptText);

  // Load whatever the cell's existing rule says, so the panel edits the rule
  // rather than silently replacing its wording with blanks on Apply.
  try {
    const j = wasm.session_validation_messages(state.sheet, s0.r0, s0.c0);
    if (j) {
      const m = JSON.parse(j);
      styleSel.value = m.style || "stop";
      errTitle.value = m.errorTitle || "";
      msg.value = m.errorText || "";
      promptTitle.value = m.promptTitle || "";
      promptText.value = m.promptText || "";
      hideDrop.checked = !!m.hideDropdown;
    }
  } catch {}

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
        // Wording and the dropdown flag are a second write over the rule just
        // created, so the list path gets them too — it takes no message
        // arguments of its own.
        wasm.session_set_validation_messages(
          state.sheet, s.r0, s.c0, s.r1, s.c1, styleSel.value,
          [errTitle.value, msg.value, promptTitle.value, promptText.value],
          hideDrop.checked);
      } catch (e) { statusError(errText(e)); }
      draw();
    },
    "Remove",
    () => {
      const s = effectiveRange();
      try { wasm.session_clear_validation(state.sheet, s.r0, s.c0, s.r1, s.c1); }
      catch (e) { statusError(errText(e)); }
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
      catch (e) { statusError(errText(e)); }
      draw();
    },
    "Clear",
    () => {
      const s = effectiveRange();
      try { wasm.session_clear_cf(state.sheet, s.r0, s.c0, s.r1, s.c1); }
      catch (e) { statusError(errText(e)); }
      draw();
    }
  );
  setTimeout(() => a.focus(), 0);
}

// Who new comments and replies are signed as. There is no account to read a
// name from in the browser, so it is asked for once and kept; an empty name is
// allowed and simply leaves the comment unsigned rather than blocking the edit.
function commentAuthor() {
  try { return localStorage.getItem("oc-comment-author") || ""; } catch { return ""; }
}
function setCommentAuthor(name) {
  try { localStorage.setItem("oc-comment-author", name); } catch {}
}

// The timestamp new comments carry, in the shape OOXML wants (`dT`). Produced
// here rather than in the engine so the engine stays a pure function of its
// inputs — the same edits always yield the same workbook.
function commentStamp() {
  return new Date().toISOString().replace("Z", "").slice(0, 22);
}

// "3 minutes ago" / "8 Aug 2026" — a thread is read by when things were said
// relative to now, and an absolute timestamp makes that arithmetic the reader's
// problem. The full stamp stays on the `title` for anyone who needs it.
function relativeTime(iso) {
  if (!iso) return "";
  const then = Date.parse(iso.endsWith("Z") ? iso : iso + "Z");
  if (!Number.isFinite(then)) return "";
  const secs = Math.round((Date.now() - then) / 1000);
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} minute${mins === 1 ? "" : "s"} ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} day${days === 1 ? "" : "s"} ago`;
  return new Date(then).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

function readThread(row, col) {
  try { return JSON.parse(wasm.session_comment_thread(state.sheet, row, col)); }
  catch { return null; }
}

function buildNotePanel(body) {
  const addrEl = el("div", "panel-range", A1(state.sel.row, state.sel.col));
  body.appendChild(addrEl);

  const thread = el("div", "cmt-thread");
  body.appendChild(thread);

  const ta = el("textarea", "panel-field");
  ta.rows = 3; ta.spellcheck = false;
  body.appendChild(ta);

  const who = el("input", "panel-field cmt-author");
  who.type = "text"; who.placeholder = "Your name (optional)";
  who.value = commentAuthor();
  who.addEventListener("change", () => setCommentAuthor(who.value.trim()));
  body.appendChild(who);

  const hint = el("div", "panel-hint", "");
  body.appendChild(hint);

  // `render` reads the thread back from the model after every change rather
  // than patching the DOM it just built, so the panel can never drift from
  // what was actually stored.
  const render = () => {
    const row = state.sel.row, col = state.sel.col;
    const t = readThread(row, col);
    thread.textContent = "";
    addrEl.textContent = A1(row, col);
    const entry = (e, isRoot) => {
      const box = el("div", "cmt-entry" + (isRoot ? " cmt-root" : ""));
      const head = el("div", "cmt-head");
      head.appendChild(el("span", "cmt-who", e.author || "Anonymous"));
      const when = el("span", "cmt-when", relativeTime(e.created));
      if (e.created) when.title = e.created;
      head.appendChild(when);
      box.appendChild(head);
      box.appendChild(el("div", "cmt-text", e.text));
      return box;
    };
    if (t) {
      if (t.resolved) thread.appendChild(el("div", "cmt-resolved", "Resolved"));
      thread.appendChild(entry(t, true));
      for (const r of t.replies) thread.appendChild(entry(r, false));
    } else {
      thread.appendChild(el("div", "panel-hint", "No comment on this cell yet."));
    }
    ta.placeholder = t ? "Reply…" : "Type a comment…";
    hint.textContent = t
      ? "Save replies to the thread. Edit rewrites the first comment; Delete removes the whole thread."
      : "Comments attach to the active cell. Select another cell to see its thread.";
    return t;
  };
  let current = render();

  panelNote = { ta, addrEl, render: () => { current = render(); }, cell: A1(state.sel.row, state.sel.col) };

  const actions = el("div", "panel-actions");
  const button = (label, cls, fn) => {
    const b = el("button", cls, label);
    b.addEventListener("click", fn);
    actions.appendChild(b);
    return b;
  };
  // The primary verb changes with the thread's state, because "Save" on an
  // existing thread is ambiguous — it could mean reply or rewrite, and those
  // are very different things to do to someone else's comment.
  const primary = button("Save", "primary", () => {
    const text = ta.value.trim();
    if (!text) return;
    const author = who.value.trim();
    setCommentAuthor(author);
    try {
      if (current) wasm.session_reply_comment(state.sheet, state.sel.row, state.sel.col, text, author, commentStamp());
      else wasm.session_set_comment(state.sheet, state.sel.row, state.sel.col, text, author, commentStamp());
    } catch (e) { statusError(errText(e)); return; }
    ta.value = "";
    current = render();
    refreshPanelButtons();
    draw();
  });
  const resolveBtn = button("Resolve", null, () => {
    if (!current) return;
    try { wasm.session_resolve_comment(state.sheet, state.sel.row, state.sel.col, !current.resolved); }
    catch (e) { statusError(errText(e)); return; }
    current = render();
    refreshPanelButtons();
    draw();
  });
  button("Delete", "danger", () => {
    // On a refusal the panel is left exactly as it was. Emptying the box
    // regardless said "deleted" for a comment that is still on the cell, and
    // the next redraw put it back.
    try { wasm.session_set_comment(state.sheet, state.sel.row, state.sel.col, "", "", ""); }
    catch (e) { statusError(errText(e)); return; }
    ta.value = "";
    current = render();
    refreshPanelButtons();
    draw();
  });
  body.appendChild(actions);

  function refreshPanelButtons() {
    primary.textContent = current ? "Reply" : "Save";
    resolveBtn.hidden = !current;
    resolveBtn.textContent = current && current.resolved ? "Reopen" : "Resolve";
  }
  refreshPanelButtons();
  panelNote.refresh = () => { current = render(); refreshPanelButtons(); };

  setTimeout(() => ta.focus(), 0);
}

// Insert or edit the hyperlink on the active cell.
//
// Two destinations rather than one: an external address, and a location inside
// this workbook. The schema treats them as independent — a link can carry both,
// meaning "open that document at this anchor" — so the dialog does too instead
// of making the user pick a mode.
function hyperlinkDialog() {
  const { row, col } = state.sel;
  let existing = null;
  try { existing = JSON.parse(wasm.session_hyperlink_at(state.sheet, row, col)); } catch {}
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent =
    existing ? `Edit link on ${A1(row, col)}` : `Insert link on ${A1(row, col)}`;
  body.textContent = "";

  const field = (label, value, placeholder) => {
    body.appendChild(el("div", "panel-label", label));
    const input = el("input", "panel-field");
    input.type = "text";
    input.value = value || "";
    input.placeholder = placeholder;
    body.appendChild(input);
    return input;
  };
  const target = field("Web address", existing && existing.target, "https://example.com");
  const location = field("Place in this workbook", existing && existing.location, "Sheet2!A1");
  const display = field("Text to display", existing && existing.display, "leave empty to keep the cell's own text");
  const tooltip = field("Tooltip", existing && existing.tooltip, "shown on hover");

  const row2 = el("div", "oc-confirm-actions");
  const commit = (clear) => {
    try {
      wasm.session_set_hyperlink(
        state.sheet, row, col,
        clear ? "" : target.value,
        clear ? "" : location.value,
        clear ? "" : tooltip.value,
        clear ? "" : display.value,
      );
      status.textContent = clear ? "link removed" : "link set";
    } catch (e) { statusError(errText(e)); }
    modal.hidden = true;
    draw();
  };
  if (existing) {
    const remove = el("button", "danger", "Remove link");
    remove.addEventListener("click", () => commit(true));
    row2.appendChild(remove);
  }
  const cancel = el("button", null, "Cancel");
  cancel.addEventListener("click", () => { modal.hidden = true; });
  const ok = el("button", "primary", existing ? "Update" : "Insert");
  ok.addEventListener("click", () => commit(false));
  row2.appendChild(cancel);
  row2.appendChild(ok);
  body.appendChild(row2);
  modal.hidden = false;
  setTimeout(() => target.focus(), 0);
}

// Open a link. An external target goes to the browser; an internal one is a
// navigation within the workbook, which is why they are not the same code path.
function followHyperlink(row, col) {
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

// Turn the selection into a table, or convert one back to a range.
//
// The header question is asked rather than guessed: whether the first row is a
// header decides the column names, and a wrong guess leaves every structured
// reference pointing at the wrong column — silently, since the formulas still
// resolve.
// The style names offered in the picker. A small, representative set rather
// than all sixty Excel ships: every family, and the six accents inside the
// family people actually use.
// Excel's totals-row functions. Each writes SUBTOTAL with the matching 10x
// code, so the total follows the filter rather than counting hidden rows.
const TOTALS_FUNCTIONS = [
  ["None", ""],
  ["Sum", "sum"],
  ["Average", "average"],
  ["Count", "count"],
  ["Count numbers", "countNums"],
  ["Max", "max"],
  ["Min", "min"],
  ["Std dev", "stdDev"],
  ["Var", "var"],
];

const TABLE_STYLES = [
  ["None", ""],
  ["Light blue", "TableStyleLight2"],
  ["Light green", "TableStyleLight7"],
  ["Blue", "TableStyleMedium2"],
  ["Orange", "TableStyleMedium3"],
  ["Grey", "TableStyleMedium4"],
  ["Gold", "TableStyleMedium5"],
  ["Steel", "TableStyleMedium6"],
  ["Green", "TableStyleMedium7"],
  ["Dark blue", "TableStyleDark2"],
];

// Create a table under the cursor, or open the panel on the one already there.
//
// Creation asks nothing: everything it could ask — name, style, banding,
// headers — is in the panel, live, and creating is one undo step. A modal that
// asked four questions before showing you anything was both slower and blind.
async function tableDialog() {
  let existing = null;
  try {
    existing = JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col));
  } catch {}
  if (existing) { openPanel("table"); return; }

  const r = effectiveRange();
  // A single cell means "the block around it", as Ctrl+T does — asking someone
  // to select the whole table first is work the app can do.
  let bounds = r;
  if (r.r0 === r.r1 && r.c0 === r.c1) {
    try {
      const blk = JSON.parse(wasm.session_block_bounds(state.sheet, r.r0, r.c0));
      if (blk) bounds = { r0: blk.r0, c0: blk.c0, r1: blk.r1, c1: blk.c1 };
    } catch {}
  }
  try {
    const name = wasm.session_create_table(
      state.sheet, bounds.r0, bounds.c0, bounds.r1, bounds.c1, "", true);
    select(bounds.r0, bounds.c0);
    status.textContent = `created ${name} — Esc closes the panel`;
  } catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  openPanel("table");
}

// A yes/no question in the shared modal. Resolves true only on the confirm
// button; Escape, the ✕ and the backdrop all mean "no", because this is only
// used to guard destructive steps.
function confirmModal(title, message, confirmLabel = "OK") {
  return new Promise((resolve) => {
    const modal = byId("oc-modal");
    const body = byId("oc-modal-body");
    byId("oc-modal-title").textContent = title;
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
    const x = byId("oc-modal-x");
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
// AutoSum: write =SUM(…) over the run of numbers directly above the cursor, or
// to its left when there is nothing above.
//
// Excel guesses the range and is right nearly every time, which is what makes
// the shortcut worth having; an AutoSum that made you select the range first
// would just be typing SUM with extra steps.
function autoSum() {
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
  ocOverlayHost.appendChild(tipEl);
  // Promote existing titles to data-tip so the native bubble doesn't also show.
  for (const node of qsa(".toolbar [title], .app-header [title], .formula-bar [title], .side-panel [title]")) {
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
// Set a control's tooltip after boot, through whichever surface it is using.
//
// `tipify` moves `title` onto `data-tip` and *deletes the attribute*, so code
// that later assigns `node.title` does two wrong things at once: the custom
// tooltip goes on showing the text it captured at boot, and the native bubble
// — the thing tipify exists to suppress — reappears underneath it. Undo and
// Redo are the only tooltips in the editor that change after boot, so they were
// the only ones that could show this, and they did: both stayed "Undo (Ctrl+Z)"
// for the life of the page instead of naming the edit they would reverse.
function setTip(node, text) {
  if (node.dataset.tip !== undefined) node.dataset.tip = text;
  else node.title = text;
  node.setAttribute("aria-label", text);
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
  const menu = byId("border-menu");
  menu.textContent = "";
  const grid = el("div", "bd-grid");
  for (const kind of ["all", "inner", "outer", "horizontal", "vertical", "none",
                      "top", "bottom", "left", "right",
                      "topandbottom", "bottomdouble", "bottomthick",
                      "diagdown", "diagup", "diagboth", "nodiag"]) {
    const b = el("button", "bd-cell");
    b.title = BD_TITLES[kind];
    b.setAttribute("aria-label", BD_TITLES[kind]);
    // oc-safe-html: `bdIcon` returns one of a fixed set of literal SVGs.
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
      const btn = byId("tb-border");
      btn.style.setProperty("--oc-x-border-swatch", c ? "#" + c : "currentColor");
    });
    sw.appendChild(b);
  }
  menu.appendChild(sw);
}
// Delete key / "Clear contents": clear values + formulas, keep formatting.
//
// All three of these are refusable — `guard_protected` rejects the range on a
// protected sheet — and all three used to swallow the refusal, so the Delete
// key on a protected sheet was a key that did nothing and said nothing. That is
// the same failure the undo path was fixed for (see `doUndo`).
function clearSelection() {
  try { for (const s of allRanges()) wasm.session_clear_contents(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { statusError(errText(e)); }
  draw();
}
// "Clear formats": drop styling, keep values + formulas.
function clearFormats() {
  try { for (const s of allRanges()) wasm.session_clear_formats(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { statusError(errText(e)); }
  draw();
}
// "Clear all": also drop styles.
function clearAll() {
  try { for (const s of allRanges()) wasm.session_clear_range(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { statusError(errText(e)); }
  draw();
}
// --- Find & replace -------------------------------------------------------
let findBar;
let findInput;
let replaceInput;
let findCount;
let findCase;
let findWhole;
let findValues;
let findAllSheets;
let findWildcards;
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
  } catch (e) { statusError(errText(e)); }
  runFind();
}
// Replace only the current match, then re-search and jump to the next one.
function replaceOne() {
  const m = findState.matches[findState.idx];
  if (!m || !findInput.value) return;
  try {
    const did = wasm.session_replace_at(state.sheet, m.r, m.c, findInput.value, replaceInput.value, findCase.checked);
    status.textContent = did ? "replaced 1" : "no match here";
  } catch (e) { statusError(errText(e)); }
  runFind();
}

// Undo/redo can add, remove, or reorder sheets, so rebuild the tab bar (which
// also re-clamps the active sheet if it vanished) before redrawing the grid.
//
// The failure is **said out loud**, where it used to be swallowed by a bare
// `catch {}`. A collaborative undo can now be refused — undoing an insert that
// somebody else has since filled would delete their work, and no undo stack
// anywhere holds it (docs/69). A refusal nobody sees is a button that appears
// to do nothing, which is the worse of the two failures that policy chose
// between, and it would have arrived silently through this line.
function doUndo() {
  try {
    wasm.session_undo();
  } catch (e) {
    statusError(errText(e));
  }
  renderTabs();
  draw();
}
function doRedo() {
  try {
    wasm.session_redo();
  } catch (e) {
    statusError(errText(e));
  }
  renderTabs();
  draw();
}
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
async function doSaveDelimited(delim, ext) {
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
  return true;
}
// Download in the format the session was **opened from**, under that format's
// own extension and content type.
//
// The engine has known all three since `WOPI-05` — `session_save_native`
// follows the format, `session_format` and `session_format_content_type` name
// it, and `session_save_loss` says what it cannot carry — and nothing on this
// page asked any of them (`WASM-01`). So a `.csv` could only leave the editor
// through an explicit "CSV" export that writes whichever tab is in front and
// labels every delimited file `text/csv`, which is the wrong type for two of
// the three.
async function doSaveNative() {
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
  status.textContent = "downloaded ." + ext;
  return true;
}
async function saveAs(fmt) {
  try {
    if (fmt === "native") { await doSaveNative(); return; }
    if (fmt === "xlsx") { doSave(); status.textContent = "downloaded .xlsx"; return; }
    const delim = fmt === "csv" ? 44 : fmt === "tsv" ? 9 : 124;
    if (await doSaveDelimited(delim, fmt)) status.textContent = "downloaded ." + fmt;
  } catch (e) { statusError(errText(e)); }
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
  // Tell the *engine*, not just the animation. Clearing the marquee alone left
  // a pending cut armed, so Esc — then a click elsewhere, then Ctrl+V — still
  // moved the data and emptied the source the user believed they had spared.
  // The visible signal said cancelled and the state said otherwise, which is
  // the worst possible pairing for an action that deletes.
  // Swallowed because this is teardown, not a command: it also runs from File
  // ▸ New and from a load, where the session it would clear is being replaced
  // anyway, and a message there would name a failure the user did not cause.
  try { wasm.session_clip_clear(); } catch {}
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
/// Parse clipboard `text/html` into the cells the engine applies.
///
/// See `docs/68-CLIPBOARD-HTML-PASTE.md`. The short version: `DOMParser` gives a
/// document with **no browsing context**, so nothing in it executes and nothing
/// in it fetches; the nodes are never inserted anywhere; and only `textContent`
/// and a fixed list of attributes are read, so `href`, `src` and every `on*`
/// handler are not consulted at all. A value that is never read cannot leak.
///
/// Returns `null` when the HTML holds no table, which is how a paste of ordinary
/// rich text falls through to the plain-text path.
function cellsFromClipboardHtml(html) {
  let doc;
  try {
    doc = new DOMParser().parseFromString(html, "text/html");
  } catch {
    return null;
  }
  const table = doc.querySelector("table");
  if (!table) return null;

  const hex = (value) => {
    if (!value) return null;
    const text = String(value).trim().toLowerCase();
    let m = /^#?([0-9a-f]{6})$/.exec(text);
    if (m) return m[1].toUpperCase();
    m = /^#?([0-9a-f]{3})$/.exec(text);
    if (m) return m[1].split("").map((c) => c + c).join("").toUpperCase();
    m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/.exec(text);
    if (m) {
      return [1, 2, 3]
        .map((i) => Math.min(255, Number(m[i])).toString(16).padStart(2, "0"))
        .join("")
        .toUpperCase();
    }
    // Named and functional colours are dropped rather than guessed: a wrong
    // fill is louder than a missing one.
    return null;
  };

  // `style` is read as text and split on `;`. Deliberately not via
  // `getComputedStyle`, which would need the node in a live document — the one
  // thing this must never do.
  const declarations = (el) => {
    const out = {};
    for (const part of (el.getAttribute("style") ?? "").split(";")) {
      const at = part.indexOf(":");
      if (at < 0) continue;
      out[part.slice(0, at).trim().toLowerCase()] = part.slice(at + 1).trim();
    }
    return out;
  };

  const cells = [];
  const rows = [...table.querySelectorAll("tr")];
  // Rows and columns are counted, because a cell that spans rows pushes the
  // cells below it sideways. Without this a merged header silently shifts every
  // row under it one column left.
  const taken = new Map();
  rows.forEach((tr, r) => {
    let c = 0;
    for (const td of tr.children) {
      if (!/^(td|th)$/i.test(td.tagName)) continue;
      while (taken.get(`${r},${c}`)) c += 1;
      const style = { ...declarations(td.closest("tr") ?? td), ...declarations(td) };
      // Producers still wrap runs in <font>/<b>/<i> inside the cell; those carry
      // formatting for the whole cell in practice.
      const inner = td.querySelector("font, span, b, i, u, s, strike");
      const innerStyle = inner ? declarations(inner) : {};
      const merged = { ...style, ...innerStyle };
      const weight = merged["font-weight"] ?? "";
      const decoration = `${merged["text-decoration"] ?? ""} ${merged["text-decoration-line"] ?? ""}`;
      const align = (merged["text-align"] ?? td.getAttribute("align") ?? "").toLowerCase();
      const valign = (merged["vertical-align"] ?? td.getAttribute("valign") ?? "").toLowerCase();
      const wrap = (merged["white-space"] ?? "").toLowerCase();
      const size = /^(\d+(?:\.\d+)?)pt/.exec(merged["font-size"] ?? "");
      const rs = Math.max(1, Number(td.getAttribute("rowspan") ?? 1) || 1);
      const cs = Math.max(1, Number(td.getAttribute("colspan") ?? 1) || 1);

      cells.push({
        dr: r,
        dc: c,
        rs,
        cs,
        text: (td.textContent ?? "").replace(/ /g, " ").trim(),
        bold:
          !!td.querySelector("b, strong") ||
          weight === "bold" ||
          (Number(weight) >= 600),
        italic: !!td.querySelector("i, em") || (merged["font-style"] ?? "").includes("italic"),
        underline: !!td.querySelector("u") || decoration.includes("underline"),
        strike: !!td.querySelector("s, strike, del") || decoration.includes("line-through"),
        wrap: wrap === "normal" || wrap === "pre-wrap",
        color: hex(merged.color),
        fill: hex(merged["background-color"] ?? merged.background ?? td.getAttribute("bgcolor")),
        font: (merged["font-family"] ?? "").split(",")[0].replace(/["']/g, "").trim() || null,
        sizeHp: size ? Math.round(Number(size[1]) * 2) : null,
        align: ["left", "center", "right", "justify"].includes(align) ? align : null,
        valign: ["top", "middle", "bottom"].includes(valign) ? valign : null,
        // Excel and LibreOffice each carry the number format in their own
        // non-standard property. Neither is guessed at from the text.
        numberFormat:
          merged["mso-number-format"]?.replace(/\\/g, "").replace(/^"|"$/g, "") ??
          td.getAttribute("sdnum")?.split(";").pop() ??
          null,
      });

      for (let dr = 0; dr < rs; dr += 1) {
        for (let dc = 0; dc < cs; dc += 1) taken.set(`${r + dr},${c + dc}`, true);
      }
      c += cs;
    }
  });
  return cells.length ? cells : null;
}

/// Read the clipboard's HTML flavour, if the browser will give it to us.
async function clipboardHtml(event) {
  const fromEvent = event?.clipboardData?.getData("text/html");
  if (fromEvent) return fromEvent;
  try {
    for (const item of await navigator.clipboard.read()) {
      if (item.types.includes("text/html")) return await (await item.getType("text/html")).text();
    }
  } catch {}
  return "";
}

async function doPaste(event) {
  try {
    let osText = event?.clipboardData?.getData("text/plain") ?? "";
    if (!osText) { try { osText = await navigator.clipboard.readText(); } catch {} }
    // Internal rich paste when the OS clipboard is unchanged from our copy (or
    // unreadable but we hold a snapshot); else paste the external text.
    if (wasm.session_clip_has() && (osText === lastClipTsv || osText === "")) {
      wasm.session_clip_paste(state.sheet, state.sel.row, state.sel.col);
    } else {
      // From another application. Prefer the HTML flavour, which is the only
      // one carrying formatting — the plain text is the same grid with every
      // style thrown away, which is what this used to be able to do.
      const cells = cellsFromClipboardHtml(await clipboardHtml(event));
      if (cells) {
        wasm.session_paste_html(state.sheet, state.sel.row, state.sel.col, JSON.stringify(cells));
      } else {
        wasm.session_paste_tsv(state.sheet, state.sel.row, state.sel.col, osText);
      }
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

function beginEdit(surface, initial, caretAtEnd = false) {
  // A read-only session refuses the write anyway; refusing here means the user
  // is told before typing rather than after, which is the difference between a
  // mode and a trap.
  if (readOnly()) {
    statusError("this workbook is open for reading only");
    return;
  }
  // A pivot's report is written by the engine and rewritten on every refresh.
  // Excel refuses the edit rather than letting a typed value stand until an
  // unrelated action wipes it, and so does this — a value that survives only
  // until something else erases it is worse than one never accepted.
  const owner = pivotBlocks(state.sel.row, state.sel.col);
  if (owner) {
    statusError(`this cell is part of the pivot table “${owner}” — change it in the fields panel`);
    return;
  }
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
  // **`preventScroll`, and it is not a nicety.**
  //
  // The grid scrolls *virtually*: the canvas is one screen tall and draws
  // whatever `state.scrollX/scrollY` say. The inline editor, though, is a real
  // `<textarea>` inside `.grid-wrap` — and `overflow: hidden` stops a *user*
  // scrolling that container, not the browser. Focusing a descendant it
  // considers out of view makes it scroll the container to reveal it, and that
  // native `scrollTop` is pure corruption: the canvas keeps drawing from
  // `state.scrollY` while the element it lives in has moved underneath it.
  //
  // The symptom was reported as "typing =2*A2 scrolled the canvas somewhere
  // weird". It is worse than a scroll — the column headers leave the screen,
  // only a band of rows draws, and the editor box detaches and floats near the
  // bottom of the window, because everything is offset twice.
  surface.focus({ preventScroll: true });
  // Typing a character starts a fresh value; opening the editor selects what is
  // already there so the next keystroke replaces it — except under F2, which
  // exists precisely to *amend* the value, so it puts the caret at the end.
  if (initial === undefined) {
    if (caretAtEnd) surface.setSelectionRange(surface.value.length, surface.value.length);
    else surface.select();
  }
  // Opening an existing formula highlights the cells it reads straight away,
  // rather than waiting for the first keystroke.
  updateRefSpans();
  updateCellMode();
  // The others learn about this edit at its first character rather than at its
  // first *change*: typing a letter on the grid sets the value programmatically,
  // which fires no `input` event, so the opening keystroke would otherwise be
  // the one nobody else ever saw.
  announceCollabSelection();
}

function startInline(initial, caretAtEnd = false) {
  beginEdit(inline, initial, caretAtEnd);
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
  paintRefTokens();
  for (const surface of [inline, fInput]) {
    const m = refMirrors.get(surface);
    if (m) { m.textContent = ""; m.style.display = "none"; }
    surface.classList.remove("tinted");
  }
  inline.style.display = "none";
  hideAutocomplete();
  hideSignatureTip();
  formulaRefDrag = null;
  pointMode = null;
  inline.classList.remove("invalid");
  fInput.classList.remove("invalid");
  if (refocus && was) canvas.focus();
  updateCellMode();
  // However the edit ended — committed, escaped, or handed to another cell —
  // this participant is no longer typing, and the others are told so by a
  // presence entry with no draft in it. There is no separate "stopped" message
  // to lose, which is what makes an abandoned edit cost nothing to clean up.
  announceCollabSelection();
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
// --- Trace precedents / dependents ----------------------------------------
//
// "Why is this cell wrong?" is usually answered by seeing what it reads, or what
// reads it. The blocks come from the same walk the recalculator uses, so an
// arrow can never point somewhere recalculation would not follow.
let traceBlocks = [];   // [{s,r0,c0,r1,c1}] on the active sheet
let traceMode = null;   // "prec" | "dep"

function toggleTrace(mode) {
  if (traceMode === mode) { clearTrace(); return; }
  traceMode = mode;
  try {
    traceBlocks = JSON.parse(
      wasm.session_trace(state.sheet, state.sel.row, state.sel.col, mode === "dep"),
    );
  } catch { traceBlocks = []; }
  // Only this sheet's blocks can be drawn; a cross-sheet arrow has nowhere to
  // point, so it is reported rather than silently dropped.
  const here = traceBlocks.filter((b) => b.s === state.sheet);
  const elsewhere = traceBlocks.length - here.length;
  traceBlocks = here;
  const n = traceBlocks.length;
  const what = mode === "dep"
    ? (n === 1 ? "dependent" : "dependents")
    : (n === 1 ? "precedent" : "precedents");
  status.textContent = n
    ? `${n} ${what}${elsewhere ? ` (+${elsewhere} on other sheets)` : ""}`
    : `no ${mode === "dep" ? "dependents" : "precedents"}`;
  draw();
}

function clearTrace() {
  if (!traceMode && !traceBlocks.length) return;
  traceMode = null;
  traceBlocks = [];
  draw();
}

// Draw an arrow from each traced block to the active cell (or the reverse for
// dependents), plus a box around the block itself.
function drawTraceArrows(withQuad) {
  if (!traceBlocks.length) return;
  const tx = colXAt(state.sel.col), ty = rowYAt(state.sel.row);
  if (tx === undefined || ty === undefined) return;
  const tcx = tx + colWAt(state.sel.col) / 2, tcy = ty + rowHAt(state.sel.row) / 2;
  ctx.save();
  ctx.strokeStyle = traceMode === "dep" ? "#a142f4" : "#1a73e8";
  ctx.fillStyle = ctx.strokeStyle;
  ctx.lineWidth = 1.5;
  for (const b of traceBlocks) {
    const x = colXAt(b.c0), y = rowYAt(b.r0);
    if (x === undefined || y === undefined) continue;
    const x1 = (colXAt(b.c1) ?? x) + colWAt(b.c1);
    const y1 = (rowYAt(b.r1) ?? y) + rowHAt(b.r1);
    withQuad(b.r0, b.c0, () => {
      ctx.strokeRect(x + 0.5, y + 0.5, x1 - x - 1, y1 - y - 1);
      // Arrow from the block's centre to the traced cell, or the other way for
      // dependents — the direction *is* the information.
      const bcx = (x + x1) / 2, bcy = (y + y1) / 2;
      const [fx, fy, txx, tyy] = traceMode === "dep"
        ? [tcx, tcy, bcx, bcy]
        : [bcx, bcy, tcx, tcy];
      ctx.beginPath();
      ctx.moveTo(fx, fy);
      ctx.lineTo(txx, tyy);
      ctx.stroke();
      const ang = Math.atan2(tyy - fy, txx - fx);
      ctx.beginPath();
      ctx.moveTo(txx, tyy);
      ctx.lineTo(txx - 8 * Math.cos(ang - 0.4), tyy - 8 * Math.sin(ang - 0.4));
      ctx.lineTo(txx - 8 * Math.cos(ang + 0.4), tyy - 8 * Math.sin(ang + 0.4));
      ctx.closePath();
      ctx.fill();
    });
  }
  ctx.restore();
}

// --- Reference tinting in the text ----------------------------------------
//
// The grid outlines say *where* each reference points; this says *which* piece
// of the formula each outline belongs to, by tinting the reference tokens in
// the same colours. A plain `<input>`/`<textarea>` cannot colour a substring, so
// a mirror element sits exactly behind the editing surface rendering the same
// text with coloured spans, and the surface itself draws its text transparent
// while keeping its caret. The mirror is inert — no pointer events, hidden from
// assistive tech — so selection, IME and every key behave as they did.
const refMirrors = new WeakMap();

function mirrorFor(surface) {
  let m = refMirrors.get(surface);
  if (m) return m;
  m = document.createElement("div");
  m.className = "ref-mirror";
  m.setAttribute("aria-hidden", "true");
  // Inserted as a sibling so it shares the surface's containing block.
  surface.parentNode.insertBefore(m, surface);
  refMirrors.set(surface, m);
  return m;
}

// Copy the metrics that decide where each glyph lands. Getting any of these
// wrong shows up immediately as text that drifts out of register with the caret.
function syncMirrorBox(surface, m) {
  const cs = getComputedStyle(surface);
  for (const prop of [
    "fontFamily", "fontSize", "fontWeight", "fontStyle", "letterSpacing",
    "lineHeight", "textIndent", "paddingTop", "paddingRight", "paddingBottom",
    "paddingLeft", "borderTopWidth", "borderRightWidth", "borderBottomWidth",
    "borderLeftWidth", "boxSizing", "whiteSpace", "textAlign",
  ]) m.style[prop] = cs[prop];
  m.style.left = surface.offsetLeft + "px";
  m.style.top = surface.offsetTop + "px";
  m.style.width = surface.offsetWidth + "px";
  m.style.height = surface.offsetHeight + "px";
}

function paintRefTokens() {
  const surface = editSurface;
  if (!surface) return;
  const m = refMirrors.get(surface);
  // Nothing to tint: drop the mirror's content and give the surface its own
  // text back, so a plain value never depends on the mirror being right.
  if (!refSpans.length) {
    if (m) { m.textContent = ""; m.style.display = "none"; }
    surface.classList.remove("tinted");
    return;
  }
  const mirror = mirrorFor(surface);
  syncMirrorBox(surface, mirror);
  mirror.style.display = "";
  surface.classList.add("tinted");

  const text = surface.value;
  // Spans are character offsets from the engine's scanner, in order.
  const parts = [];
  let at = 0;
  refSpans.forEach((r, i) => {
    const start = Math.max(at, Math.min(r.s, text.length));
    const end = Math.max(start, Math.min(r.e, text.length));
    if (start > at) parts.push([text.slice(at, start), null]);
    parts.push([text.slice(start, end), REF_COLORS[i % REF_COLORS.length]]);
    at = end;
  });
  parts.push([text.slice(at), null]);
  mirror.textContent = "";
  for (const [chunk, color] of parts) {
    if (!chunk) continue;
    const node = document.createElement("span");
    node.textContent = chunk;
    if (color) node.style.color = color;
    mirror.appendChild(node);
  }
  // A long formula scrolls inside the surface; the mirror has to follow or the
  // tint slides off the tokens it belongs to.
  mirror.scrollLeft = surface.scrollLeft;
  mirror.scrollTop = surface.scrollTop;
}

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
  // The text can change without the *set* of references changing (typing inside
  // a token, or anywhere else in the formula), so the tint is repainted every
  // time while only the grid outlines are gated on a real change.
  paintRefTokens();
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
  // Every path that changes the text of an open edit passes through here — the
  // keystroke handler, reference insertion, autocomplete, the anchor cycle — so
  // this is where the others find out what is being typed. Hooking the `input`
  // event instead would miss every programmatic change, which is most of the
  // interesting ones in a formula.
  announceCollabSelection();
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
// The structural tree is `rebuildA11yGrid` below; this is the running
// commentary beside it. A live region is what announces a *change* — moving the
// selection, growing it — which a static tree cannot do on its own.
let liveEl;
let modeEl;
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

// --- The accessibility tree ------------------------------------------------
//
// A DOM mirror of the cells currently on screen. The canvas cannot expose
// anything structurally, so without this a screen reader has a live region and
// nothing else: no way to read across a row, no column headers, no sense of
// where in the sheet you are.
//
// Only the visible window is mirrored — a million cells of DOM would defeat the
// point of drawing to a canvas at all. That is invisible to the reader because
// every cell carries its **absolute** `aria-rowindex`/`aria-colindex` against
// the sheet's declared counts, which is what those attributes exist for: "row
// 4,201 of 1,048,576" stays true when only rows 4,190–4,230 are in the DOM.
//
// It mirrors `geo.rowIdx`/`geo.colIdx` — the indices the renderer just drew —
// so a hidden or filtered-out row is absent from the mirror for the same reason
// it is absent from the screen, without a second notion of what is visible.
let a11yEl;
let a11ySignature = "";

// Caps so an unusually large window cannot make the rebuild expensive. Past the
// cap the reader still gets the cells nearest the top-left of the view, which is
// where navigation is.
const A11Y_MAX_ROWS = 60;
const A11Y_MAX_COLS = 40;

const a11yCellId = (row, col) => `a11y-${row}-${col}`;

function rebuildA11yGrid() {
  if (!a11yEl || !wasm || !geo.rowIdx || !geo.colIdx) return;
  const rows = geo.rowIdx.slice(0, A11Y_MAX_ROWS);
  const cols = geo.colIdx.slice(0, A11Y_MAX_COLS);
  if (!rows.length || !cols.length) return;

  const sel = selRect();
  const active = a11yCellId(state.sel.row, state.sel.col);

  // Rebuilding identical DOM every frame churns the accessibility tree and
  // makes some readers re-announce the whole grid, so only rebuild when what
  // the mirror shows has actually changed. The cell payload is part of the
  // signature, so an edit refreshes it without needing an edit counter.
  const signature = [
    state.sheet, rows[0], rows[rows.length - 1], cols[0], cols[cols.length - 1],
    sel.r0, sel.c0, sel.r1, sel.c1, geoItems.length,
  ].join(",") + "|" + JSON.stringify(geoItems);
  if (signature === a11ySignature) {
    canvas.setAttribute("aria-activedescendant", active);
    return;
  }
  a11ySignature = signature;

  const byKey = new Map();
  for (const it of geoItems) byKey.set(it.r + "," + it.c, it.t);

  const frag = document.createDocumentFragment();
  // A header row of column letters, so "column D" is something the reader can
  // say rather than something the user has to count to.
  const head = el("div", null);
  head.setAttribute("role", "row");
  head.setAttribute("aria-rowindex", "1");
  const corner = el("div", null, "");
  corner.setAttribute("role", "columnheader");
  corner.setAttribute("aria-colindex", "1");
  head.appendChild(corner);
  for (const c of cols) {
    const h = el("div", null, colName(c));
    h.setAttribute("role", "columnheader");
    // +2: the row-header column is index 1, and ARIA indices are 1-based.
    h.setAttribute("aria-colindex", String(c + 2));
    head.appendChild(h);
  }
  frag.appendChild(head);

  for (const r of rows) {
    const row = el("div", null);
    row.setAttribute("role", "row");
    // +2: the column-header row is index 1.
    row.setAttribute("aria-rowindex", String(r + 2));
    const rh = el("div", null, String(r + 1));
    rh.setAttribute("role", "rowheader");
    rh.setAttribute("aria-colindex", "1");
    row.appendChild(rh);
    for (const c of cols) {
      const text = byKey.get(r + "," + c);
      const cell = el("div", null, text || "");
      cell.id = a11yCellId(r, c);
      cell.setAttribute("role", "gridcell");
      cell.setAttribute("aria-colindex", String(c + 2));
      // A cell with no accessible name is skipped outright by some readers, so
      // an empty one says it is empty instead of vanishing from the row and
      // making the columns misalign as they are read across.
      if (!text) cell.setAttribute("aria-label", `${A1(r, c)} empty`);
      if (r >= sel.r0 && r <= sel.r1 && c >= sel.c0 && c <= sel.c1) {
        cell.setAttribute("aria-selected", "true");
      }
      row.appendChild(cell);
    }
    frag.appendChild(row);
  }

  a11yEl.textContent = "";
  a11yEl.appendChild(frag);
  canvas.setAttribute("aria-activedescendant", active);
}

// The counts a reader phrases "row 12 of N" against.
//
// Deliberately the **navigable** extent, not the used range: the same
// `+30 rows / +8 columns` past the data that the scrollbars offer. Counting
// only used rows would announce "row 40 of 6" the moment anyone scrolled below
// the data, since the mirror covers the screen and the screen goes further than
// the data does. The +1 on each is the header row and column, which are part of
// the grid as far as ARIA is concerned.
function updateGridCounts() {
  if (!wasm) return;
  try {
    const b = usedBounds();
    const lastRow = geo.rowIdx?.length ? geo.rowIdx[geo.rowIdx.length - 1] + 1 : 0;
    const lastCol = geo.colIdx?.length ? geo.colIdx[geo.colIdx.length - 1] + 1 : 0;
    canvas.setAttribute("aria-rowcount", String(Math.max(b.rows + 30, lastRow) + 1));
    canvas.setAttribute("aria-colcount", String(Math.max(b.cols + 8, lastCol) + 1));
  } catch {}
}

// Excel's status-bar mode word. Ready → Enter (typing a fresh value) → Edit
// (F2 into an existing one) → Point (picking a reference mid-formula).
function updateCellMode() {
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

// True while the selection is being extended from the keyboard; cleared the
// moment the selection is set outright.
let extending = false;

function updateNameBox() {
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
// Parse what the Name Box accepts into a selection box: `B7`, `A1:C9`, a whole
// column band `A:C`, or a whole row band `2:5`. Returns null if it is not one of
// those — a defined name, most likely, which the caller tries next.
function parseNameRange(text) {
  const t = (text || "").trim();
  if (!t) return null;
  const b = usedBounds();
  const wholeCols = /^\$?([A-Za-z]{1,3})\s*:\s*\$?([A-Za-z]{1,3})$/.exec(t);
  if (wholeCols) {
    const a = colFromName(wholeCols[1]), z = colFromName(wholeCols[2]);
    if (a === null || z === null) return null;
    return { r0: 0, c0: Math.min(a, z), r1: Math.max(0, b.rows - 1), c1: Math.max(a, z), kind: "cols" };
  }
  const wholeRows = /^\$?(\d+)\s*:\s*\$?(\d+)$/.exec(t);
  if (wholeRows) {
    const a = parseInt(wholeRows[1], 10) - 1, z = parseInt(wholeRows[2], 10) - 1;
    if (a < 0 || z < 0) return null;
    return { r0: Math.min(a, z), c0: 0, r1: Math.max(a, z), c1: Math.max(0, b.cols - 1), kind: "rows" };
  }
  const parts = t.split(":");
  if (parts.length === 2) {
    const p = parseA1Cell(parts[0]), q = parseA1Cell(parts[1]);
    if (!p || !q) return null;
    return {
      r0: Math.min(p.row, q.row), c0: Math.min(p.col, q.col),
      r1: Math.max(p.row, q.row), c1: Math.max(p.col, q.col),
    };
  }
  const c = parseA1Cell(t);
  return c ? { r0: c.row, c0: c.col, r1: c.row, c1: c.col } : null;
}

// Column letters to a zero-based index, or null.
function colFromName(letters) {
  let n = 0;
  for (const ch of letters.toUpperCase()) {
    const v = ch.charCodeAt(0) - 64;
    if (v < 1 || v > 26) return null;
    n = n * 26 + v;
  }
  return n - 1;
}

// Delimited text arrives in whatever encoding produced it. The engine reads
// UTF-8, so anything else has to be converted here — a UTF-16 export opened as
// UTF-8 is not slightly wrong, it is unreadable.
function decodeTextBytes(bytes) {
  const enc = (label) => new TextEncoder().encode(new TextDecoder(label).decode(bytes));
  // Byte-order marks are definitive, so they are checked before anything else.
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) return enc("utf-16le");
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) return enc("utf-16be");
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return bytes.slice(3); // UTF-8 BOM: strip it, the rest is already UTF-8
  }
  // No BOM. A UTF-16 file without one still gives itself away: half its bytes
  // are zero. Sniffing beats failing, and the fallback is plain UTF-8.
  const probe = bytes.subarray(0, Math.min(bytes.length, 512));
  let zeros = 0;
  for (const b of probe) if (b === 0) zeros += 1;
  if (probe.length > 8 && zeros > probe.length / 4) {
    return enc(bytes[0] === 0 ? "utf-16be" : "utf-16le");
  }
  return bytes;
}

// Turn an engine error into something that says what to do about it.
function friendlyOpenError(err, name, isText) {
  const text = String(err && err.message ? err.message : err);
  // Checked first, and by code: a stopped open is not a complaint about the
  // file, and telling someone their perfectly good workbook is unreadable
  // because it was large is worse than saying nothing.
  if (wasStopped(err)) {
    return `${name} was taking too long, so it was stopped — nothing was loaded`;
  }
  if (/is not a format this build can open/.test(text)) return text;
  if (/zip|central directory|not a valid/i.test(text)) {
    return `${name} is not a readable .xlsx — if it is an older .xls, re-save it as .xlsx first`;
  }
  if (/limit|too (large|many)|bound/i.test(text)) {
    return `${name} exceeds this build's size limits and was not opened`;
  }
  if (/utf-?8|invalid|encoding/i.test(text) && isText) {
    return `${name} is not text this build can decode — try saving it as UTF-8 CSV`;
  }
  return `could not open ${name}: ${text}`;
}

// Anything the importer had to drop or degrade, said once, plainly. The report
// exists in the engine; nothing surfaced it, so a lossy import looked clean.
function reportImportIssues() {
  let summary = "";
  try { summary = wasm.session_import_summary(); } catch {}
  if (!summary) return;
  const bar = byId("tb-status");
  // The summary names parts of the file that did not survive import, so it
  // quotes the workbook — sheet names, defined names, function names. Re-parsing
  // `bar.textContent` as markup made that a second injection point on top of
  // the first.
  const warn = document.createElement("span");
  warn.className = "warn";
  warn.textContent = summary;
  bar.replaceChildren(document.createTextNode(`${bar.textContent} — `), warn);
}

// Fill the selection from its own first row or column, in an explicit mode.
// Which axis is decided by the selection's shape: a tall block fills down, a
// wide one fills right — the same reading the fill handle gives it.
function fillSelection(mode) {
  const s = effectiveRange();
  const rows = s.r1 - s.r0 + 1, cols = s.c1 - s.c0 + 1;
  if (rows < 2 && cols < 2) { status.textContent = "select the cells to fill"; return; }
  const down = rows >= cols;
  const src = down
    ? { r0: s.r0, c0: s.c0, r1: s.r0, c1: s.c1 }
    : { r0: s.r0, c0: s.c0, r1: s.r1, c1: s.c0 };
  tryEdit(() => wasm.session_fill_mode(
    state.sheet, src.r0, src.c0, src.r1, src.c1, s.r0, s.c0, s.r1, s.c1, mode));
  lastFill = { src, dst: { ...s } };
  status.textContent = `filled — ${mode}`;
}

// The fill-options button Excel drops at the corner of a fill. A fill has to
// guess between copying and continuing a series, and this is how you tell it it
// guessed wrong — without which the only recourse is undo and a different drag.
let lastFill = null;

function hideFillOptions() {
  const b = byId("fill-options");
  if (b) b.remove();
}

function showFillOptions(dst) {
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

// Paste Special (Ctrl+Alt+V): what to paste, and what to do with it when it
// lands. The context submenu covers the common three; this is the rest —
// transpose and the arithmetic combinations.
function pasteSpecialDialog() {
  if (!wasm.session_clip_has()) { status.textContent = "clipboard is empty"; return; }
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Paste special";
  body.textContent = "";

  let what = "all";
  const group = (label, options, onPick) => {
    body.append(el("p", "oc-confirm-text", label));
    const row = el("div", "fc-row");
    options.forEach(([v, t], i) => {
      const l = el("label", "fc-check");
      const r = document.createElement("input");
      r.type = "radio";
      r.name = "ps-" + label;
      r.value = v;
      if (i === 0) r.checked = true;
      r.addEventListener("change", () => onPick(v));
      l.append(r, document.createTextNode(" " + t));
      row.appendChild(l);
    });
    body.appendChild(row);
  };
  group("Paste", [
    ["all", "Everything"], ["values", "Values only"],
    ["formulas", "Formulas"], ["formats", "Formats only"],
  ], (v) => { what = v; });

  let op = "none";
  group("Operation", [
    ["none", "None"], ["add", "Add"], ["subtract", "Subtract"],
    ["multiply", "Multiply"], ["divide", "Divide"],
  ], (v) => { op = v; });

  const tWrap = el("label", "fc-check");
  const transpose = document.createElement("input");
  transpose.type = "checkbox";
  tWrap.append(transpose, document.createTextNode(" Transpose"));
  body.append(tWrap);
  body.append(el("div", "panel-hint",
    "An operation combines the copied numbers with what is already there. Non-numeric cells are left alone."));

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Paste");
  actions.append(cancel, ok);
  body.appendChild(actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    close();
    canvas.focus();
    // An arithmetic operation is what to *do*, so it wins over what to paste;
    // transpose is a placement and only applies to a plain paste.
    const mode = op !== "none" ? op : transpose.checked ? "transpose" : what;
    doPasteMode(mode);
  });
  ok.focus();
}

// Text to columns: split the selected column on a delimiter into the columns to
// its right. Runs entirely on values already in the sheet, so it needs no
// clipboard and no import path.
function textToColumnsDialog() {
  const s0 = effectiveRange();
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Text to columns";
  body.textContent = "";
  body.append(el("p", "oc-confirm-text",
    `Split column ${colName(s0.c0)}, rows ${s0.r0 + 1}–${s0.r1 + 1}, into the columns to its right.`));

  let delim = ",";
  const row = el("div", "fc-row");
  for (const [v, t] of [[",", "Comma"], ["\t", "Tab"], [";", "Semicolon"], [" ", "Space"], ["", "Custom"]]) {
    const l = el("label", "fc-check");
    const r = document.createElement("input");
    r.type = "radio"; r.name = "ttc"; r.value = v;
    if (v === ",") r.checked = true;
    r.addEventListener("change", () => { delim = v === "" ? custom.value : v; });
    l.append(r, document.createTextNode(" " + t));
    row.appendChild(l);
  }
  const custom = el("input", "panel-field");
  custom.placeholder = "delimiter";
  custom.style.maxWidth = "120px";
  custom.addEventListener("input", () => {
    const c = row.querySelector('input[value=""]');
    if (c) { c.checked = true; delim = custom.value; }
  });
  body.append(row, custom);
  const warn = el("div", "panel-hint", "");
  body.append(warn);

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Split");
  actions.append(cancel, ok);
  body.appendChild(actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    if (!delim) { warn.textContent = "Choose a delimiter first."; return; }
    close();
    canvas.focus();
    tryEdit(() => {
      let widest = 0;
      for (let r = s0.r0; r <= s0.r1; r++) {
        const text = wasm.session_cell_input(state.sheet, r, s0.c0);
        // Only literal text splits; a formula's result is not the user's text,
        // and overwriting the formula would lose it.
        if (!text || text.startsWith("=")) continue;
        const parts = text.split(delim);
        widest = Math.max(widest, parts.length);
        parts.forEach((part, i) => {
          wasm.session_set_cell(state.sheet, r, s0.c0 + i, part.trim());
        });
      }
      status.textContent = widest > 1
        ? `split into ${widest} columns`
        : "nothing to split — no cell contained the delimiter";
    });
  });
  ok.focus();
}

// Insert Function: the catalogue, searchable, with each entry's signature and
// summary. The `fx` beside the formula bar was decorative — the only way to find
// a function was to already know its name and start typing it.
function insertFunctionDialog() {
  if (!fnCatalog) { try { fnCatalog = JSON.parse(wasm.function_catalog()); } catch { fnCatalog = []; } }
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Insert function";
  body.textContent = "";

  const search = el("input", "panel-field");
  search.placeholder = "Search functions";
  search.spellcheck = false;
  const list = el("div", "fn-list");
  const detail = el("div", "panel-hint");
  body.append(search, list, detail);

  let chosen = null;
  const render = () => {
    const q = search.value.trim().toUpperCase();
    const items = (fnCatalog || []).filter((f) => !q || f.n.includes(q)).slice(0, 200);
    list.textContent = "";
    for (const f of items) {
      const b = el("button", "fn-row", f.n);
      b.addEventListener("click", () => {
        chosen = f;
        list.querySelectorAll(".fn-row").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
        detail.textContent = `${f.sig || f.n + "(…)"}${f.d ? " — " + f.d : ""}`;
      });
      b.addEventListener("dblclick", () => { chosen = f; ok.click(); });
      list.appendChild(b);
    }
    if (!items.length) list.appendChild(el("div", "panel-hint", "No matching function"));
  };
  search.addEventListener("input", render);
  render();

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Insert");
  actions.append(cancel, ok);
  body.appendChild(actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    const f = chosen || (fnCatalog || [])[0];
    close();
    if (!f) { canvas.focus(); return; }
    // Open an edit on the active cell and drop the call in with the caret
    // between the parentheses, ready for arguments.
    beginEdit(inline, "=" + f.n + "()");
    const at = inline.value.length - 1;
    inline.setSelectionRange(at, at);
    updateRefSpans();
  });
  search.focus();
}

// A caret on the Name Box listing the workbook's defined names. They are
// otherwise only reachable by typing one exactly, which means knowing it exists.
function openNameBoxList() {
  closeSheetMenu();
  let names = [];
  try { names = JSON.parse(wasm.session_names()); } catch {}
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu";
  menu.id = "sheet-ctx";
  if (!names.length) {
    menu.appendChild(el("div", "panel-hint", "No defined names yet."));
  } else {
    for (const n of names) {
      const b = el("button", "menu-item", n.name || n);
      b.addEventListener("click", () => { closeSheetMenu(); gotoName(n.name || n); canvas.focus(); });
      menu.appendChild(b);
    }
  }
  const r = cellRef.getBoundingClientRect();
  positionMenu(menu, r.left, r.bottom + 2);
}

function gotoName(v) {
  const s = (v || "").trim();
  if (!s) { updateNameBox(); return; }

  // A comma-separated list builds a multi-range selection, as in Excel.
  if (s.includes(",")) {
    const parts = s.split(",").map((x) => x.trim()).filter(Boolean);
    const boxes = parts.map(parseNameRange).filter(Boolean);
    if (boxes.length === parts.length && boxes.length > 1) {
      state.ranges = boxes.slice(0, -1);
      const last = boxes[boxes.length - 1];
      state.anchor = { row: last.r0, col: last.c0 };
      state.sel = { row: last.r1, col: last.c1 };
      state.selKind = "cells";
      ensureVisible();
      draw();
      return;
    }
  }

  // A sheet qualifier moves there first, so `Sheet2!B7` lands on Sheet2.
  let text = s;
  const bang = text.lastIndexOf("!");
  if (bang > 0) {
    const name = text.slice(0, bang).replace(/^'|'$/g, "");
    try {
      const idx = JSON.parse(wasm.session_sheet_names())
        .findIndex((n) => n.toLowerCase() === name.toLowerCase());
      if (idx >= 0) { switchSheet(idx); text = text.slice(bang + 1); }
    } catch {}
  }

  const box = parseNameRange(text);
  if (box) {
    if (box.r0 === box.r1 && box.c0 === box.c1) { select(box.r0, box.c0); return; }
    state.ranges = [];
    state.anchor = { row: box.r0, col: box.c0 };
    state.sel = { row: box.r1, col: box.c1 };
    state.selKind = box.kind || "cells";
    ensureVisible();
    draw();
    return;
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
    catch (e) { statusError(errText(e)); }
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
    // Built, not interpolated. Both of these are workbook text: a `refersTo`
    // reading `<img src=x onerror=...>` used to become a real element here, and
    // opening the Name Manager on a file somebody sent you ran their script in
    // this origin. Elements and `textContent` cannot do that.
    const label = document.createElement("b");
    label.textContent = n.name;
    const target = document.createElement("span");
    target.textContent = n.refersTo;
    go.replaceChildren(label, target);
    go.addEventListener("click", () => { closeSheetMenu(); gotoName(n.name); });
    const del = document.createElement("button");
    del.className = "nm-del";
    del.textContent = "×";
    del.title = "Delete";
    // The row only goes when the name did. Removing it regardless took the
    // entry off the list while the workbook still held it, so the next time the
    // menu was opened it was back and nobody knew why.
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      try { wasm.session_delete_name(n.name); }
      catch (err) { statusError(errText(err)); return; }
      row.remove();
      draw();
    });
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
let acEl;

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

function hideAutocomplete() { acState = null; if (acEl) acEl.hidden = true; }

// --- Argument hint ----------------------------------------------------------
// Once the caret is inside a function's parentheses the name list is no longer
// what you need — the question becomes "which argument am I typing?". This
// shows the signature with that argument emphasised, the way Excel and Sheets
// do, and follows the caret through nested calls.
let sigEl;

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

let tabsEl;

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
  // Which participants are "elsewhere" is relative to the sheet on screen, so
  // the roster is re-read whenever that changes.
  renderPresence();
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
      b.style.setProperty("--oc-x-tab-color", "#" + tc);
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
  // oc-safe-html: a literal SVG icon.
  add.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="icon-sm"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>';
  add.addEventListener("click", () => {
    try {
      const i = wasm.session_add_sheet();
      switchSheet(i);
      renderTabs();
    } catch (e) { statusError(errText(e)); }
  });
  tabsEl.appendChild(add);

  // All-sheets menu. The strip scrolls once there are more tabs than fit, but
  // scrolling only helps if you already know where you are going — this lists
  // every sheet, hidden ones included, and jumps straight to it.
  const all = document.createElement("button");
  all.className = "sheet-add sheet-all";
  all.title = "All sheets";
  all.setAttribute("aria-label", "All sheets");
  // oc-safe-html: a literal SVG icon.
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
        // A refused reveal stays put and says why: switching anyway would land
        // on the sheet with no tab that this branch exists to avoid.
        if (hiddenHere) {
          try { wasm.session_set_sheet_visibility(idx, "visible"); }
          catch (err) { statusError(errText(err)); return; }
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

function closeSheetMenu() {
  const m = byId("sheet-ctx");
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
    catch (e) { statusError(errText(e)); }
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
    catch (e) { statusError(errText(e)); }
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
    catch (e) { statusError(errText(e)); }
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
    } catch (e) { statusError(errText(e)); }
  });
  positionMenu(menu, x, y);
}

// Append a context menu at (x,y), flipping up/left if it would overflow.
function positionMenu(menu, x, y) {
  menu.style.left = "0px";
  menu.style.top = "0px";
  menu.style.visibility = "hidden";
  ocOverlayHost.appendChild(menu);
  const h = menu.offsetHeight, w = menu.offsetWidth;
  menu.style.top = (y + h > window.innerHeight ? Math.max(4, y - h) : y) + "px";
  menu.style.left = (x + w > window.innerWidth ? Math.max(4, x - w) : x) + "px";
  menu.style.visibility = "visible";
  setTimeout(() => document.addEventListener("click", closeSheetMenu, { once: true }), 0);
}

// Show or hide the selected cell's data-validation input hint.
//
// A tooltip pinned under the cell rather than a status-bar line: it belongs to
// the cell, and it has to survive the status bar being used for something else.
function refreshValidationPrompt() {
  const box = byId("dv-prompt");
  if (!box) return;
  let hint = "";
  try {
    hint = wasm ? wasm.session_validation_prompt(state.sheet, state.sel.row, state.sel.col) : "";
  } catch {}
  if (!hint || state.selKind !== "cells") { box.hidden = true; return; }
  let p;
  try { p = JSON.parse(hint); } catch { box.hidden = true; return; }
  box.textContent = "";
  if (p.title) box.appendChild(el("strong", null, p.title));
  if (p.text) box.appendChild(el("span", null, p.text));
  const x = colXAt(state.sel.col), y = rowYAt(state.sel.row);
  if (x === undefined || y === undefined) { box.hidden = true; return; }
  const rect = canvas.getBoundingClientRect();
  box.style.left = `${rect.left + x}px`;
  box.style.top = `${rect.top + y + rowHAt(state.sel.row) + 4}px`;
  box.hidden = false;
}

// A thrown value as a sentence.
//
// A `JsError` from the engine stringifies as "Error: …", so interpolating it
// after the word "error" read "error: Error: this sheet is protected".
function errText(e) {
  return String((e && e.message) || e).replace(/^Error:\s*/, "");
}

// Put a message in the status bar as an error, without going through innerHTML.
//
// The wording can come from the file — a data-validation rule carries the
// author's own text — so interpolating it into markup would let a workbook
// inject nodes into the page.
function statusError(text) {
  status.textContent = "";
  const span = document.createElement("span");
  span.className = "err";
  span.textContent = text;
  status.appendChild(span);
}

function tryEdit(fn) {
  try { fn(); } catch (e) { statusError(errText(e)); }
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
/// Whether a cell shift needs confirming before it runs.
///
/// **A probe that cannot answer means "warn", not "proceed".** This began at
/// `false` inside a swallowing `catch`, so a probe that threw asserted the
/// *safe* answer and the confirmation was skipped for precisely the shift that
/// was about to break formulas — the one case where the warning is the only
/// thing standing between the user and silent corruption (`UX-SHIFT-01`).
///
/// The cost of the two answers is not symmetric, which is the whole argument:
/// guessing "risky" wrongly costs one extra dialog, and guessing "safe" wrongly
/// costs formulas that now point at different cells with nothing on screen to
/// say so.
///
/// Takes the probe as a callback so the failing case is reachable from a test
/// without a wasm module that can be made to throw on demand.
export function shiftIsRisky(probe) {
  try {
    return Boolean(probe());
  } catch {
    return true;
  }
}

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
    ["Paste special…", false, () => pasteSpecialDialog()],
    ["Values only", false, () => doPasteMode("values")],
    ["Formulas only", false, () => doPasteMode("formulas")],
    ["Formats only", false, () => doPasteMode("formats")],
    ["Transpose", false, () => doPasteMode("transpose")],
  ]);
  sep();
  // Insert/delete *cells*, shifting the rest. References are not rewritten, so
  // the user is told when that matters rather than discovering it later.
  const shiftCells = (insert, vertical, label) => () => {
    const r = effectiveRange();
    const risky = shiftIsRisky(() =>
      wasm.session_shift_affects_formulas(state.sheet, r.r0, r.c0, r.r1, r.c1, vertical));
    const run = () => {
      tryEdit(() => wasm.session_shift_cells(state.sheet, r.r0, r.c0, r.r1, r.c1, insert, vertical));
      status.textContent = label.toLowerCase();
    };
    if (!risky) { run(); return; }
    confirmModal(
      "Formulas reference these cells",
      "Moving them will not adjust those references — they will keep pointing at the same addresses, which will now hold different cells.",
      label,
    ).then((ok) => { if (ok) run(); });
  };
  submenu("Insert cells", [
    ["Shift cells down", false, shiftCells(true, true, "Inserted, shifted down")],
    ["Shift cells right", false, shiftCells(true, false, "Inserted, shifted right")],
  ]);
  submenu("Delete cells", [
    ["Shift cells up", false, shiftCells(false, true, "Deleted, shifted up")],
    ["Shift cells left", false, shiftCells(false, false, "Deleted, shifted left")],
  ]);
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
  sep();
  // The things you reach for *from a cell* — previously only on the toolbar or
  // the menu bar, which is a long way to go for something the right-click is
  // already asking about.
  item("Format cells…", false, () => formatCellsDialog());
  // The verb reflects what is actually there, so the menu is not offering to
  // insert a comment onto a cell that already has a thread.
  item(
    readThread(state.sel.row, state.sel.col) ? "Show comments" : "Insert comment",
    false,
    () => { if (activePanel !== "note") togglePanel("note"); else panelNote?.refresh(); },
  );
  item(
    (() => {
      let has = null;
      try { has = JSON.parse(wasm.session_hyperlink_at(state.sheet, state.sel.row, state.sel.col)); } catch {}
      return has ? "Edit link…" : "Insert link…";
    })(),
    false,
    () => hyperlinkDialog(),
  );
  item(
    (() => {
      let t = null;
      try { t = JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col)); } catch {}
      return t ? "Convert to range…" : "Create table…";
    })(),
    false,
    () => tableDialog(),
  );
  (() => {
    let t = null;
    try { t = JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col)); } catch {}
    if (!t) return;
    item(t.totals > 0 ? "Hide totals row" : "Show totals row", false, () => {
      try {
        wasm.session_table_totals(state.sheet, state.sel.row, state.sel.col, t.totals === 0);
        status.textContent = t.totals > 0 ? "totals row hidden" : "totals row shown";
      } catch (e) { statusError(errText(e)); }
      draw();
    });
  })();
  item("Define name…", false, () => {
    const r = canvas.getBoundingClientRect();
    openNameManager(r.left + 120, r.top + 90);
  });
  item("Filter", false, () => toggleFilter());
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
  } catch (e) { statusError(errText(e)); }
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
  } catch (e) { statusError(errText(e)); }
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
  // The real paste event, which is the only place `clipboardData` exists.
  //
  // Reading the clipboard through `navigator.clipboard.read()` needs a
  // permission prompt and returns nothing in Firefox; the event carries every
  // flavour the source application offered, for free, because the user just
  // asked for it. That is what makes formatting recoverable at all.
  document.addEventListener("paste", (e) => {
    // Not while a cell editor or a dialog input has focus — there the browser's
    // own text paste is exactly right.
    const target = e.target;
    if (target && /^(input|textarea)$/i.test(target.tagName ?? "")) return;
    if (target?.isContentEditable) return;
    e.preventDefault();
    void doPaste(e);
  });

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
    // A plain click on a linked cell follows it, as Excel does. Guarded three
    // ways so it cannot hijack ordinary work: not while picking a formula
    // reference, not with a modifier held (Ctrl-click is how you select a
    // linked cell without leaving), and not on a second click of a cell that is
    // already selected — that is someone starting a drag or a rename, not
    // asking to navigate.
    if (!refAcceptable() && !e.ctrlKey && !e.metaKey && !e.shiftKey && e.button === 0) {
      const hit = cellAt(px, py);
      if (
        hit
        && linkCells.has(hit.row + "," + hit.col)
        && !(hit.row === state.sel.row && hit.col === state.sel.col)
      ) {
        e.preventDefault();
        select(hit.row, hit.col);
        followHyperlink(hit.row, hit.col);
        draw();
        return;
      }
    }
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
    // A chart: its frame floats over the cells, so it is tested before them or
    // a click would land on whatever is underneath and the chart could never
    // be picked up.
    if (e.button === 0 && chartMouseDown(px, py)) {
      if (editSurface && !commit(editSurface.value, false)) return;
      e.preventDefault();
      canvas.focus();
      return;
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
    const fnew = freezeHandleAt(px, py);
    if (fnew) { endInline(); state.freezeDrag = { axis: fnew.axis, px, py }; return; }
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
    if (chartDrag) { chartDrag.px = px; chartDrag.py = py; draw(); return; }
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
    const fnew = freezeHandleAt(px, py);
    canvas.style.cursor = (fnew || fh || hb)
      ? ((fnew || fh || hb).axis === "col" ? "col-resize" : "row-resize")
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
    } else if (hit && linkCells.has(hit.row + "," + hit.col)) {
      let link = null;
      try { link = JSON.parse(wasm.session_hyperlink_at(state.sheet, hit.row, hit.col)); } catch {}
      const dest = link && (link.tooltip || link.target || link.location);
      if (dest) {
        commentTip.textContent = dest;
        commentTip.style.whiteSpace = "";
        commentTip.style.left = (px + 14) + "px";
        commentTip.style.top = (py + 8) + "px";
        commentTip.hidden = false;
        canvas.style.cursor = "pointer";
      } else commentTip.hidden = true;
    } else if (hit && commentCells.has(hit.row + "," + hit.col)) {
      // The hover shows the whole thread, not just the opening remark: a reply
      // is usually the part that answers the question the cell raises, and
      // hiding it behind "open the panel" makes the indicator half-useful.
      const t = readThread(hit.row, hit.col);
      if (t && t.text) {
        const line = (e) => (e.author ? `${e.author}: ` : "") + e.text;
        const lines = [line(t), ...t.replies.map(line)];
        if (t.resolved) lines.unshift("✓ Resolved");
        commentTip.textContent = lines.join("\n");
        commentTip.style.whiteSpace = "pre-line";
        commentTip.style.left = (px + 14) + "px";
        commentTip.style.top = (py + 8) + "px";
        commentTip.hidden = false;
      } else commentTip.hidden = true;
    } else {
      commentTip.hidden = true;
    }
  });
  window.addEventListener("mouseup", (e) => {
    if (chartDrag) { chartMouseUp(); return; }
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
      } catch (e) { statusError(errText(e)); }
      draw();
    }
    if (state.fill) {
      const f = state.fill;
      state.fill = null;
      const d = f.dst;
      if (d && (d.r0 !== f.src.r0 || d.c0 !== f.src.c0 || d.r1 !== f.src.r1 || d.c1 !== f.src.c1)) {
        // Ctrl inverts the default, as in Excel: a series-looking source copies
        // instead, and a plain one becomes a series.
        const mode = (e.ctrlKey || e.metaKey) ? "copy" : "auto";
        try {
          wasm.session_fill_mode(state.sheet, f.src.r0, f.src.c0, f.src.r1, f.src.c1,
                                 d.r0, d.c0, d.r1, d.c1, mode);
          status.textContent = "filled";
        } catch (err) { statusError(errText(err)); }
        state.anchor = { row: d.r0, col: d.c0 };
        state.sel = { row: d.r1, col: d.c1 };
        state.selKind = "cells";
        // Remember the fill so the options popup can redo it differently.
        lastFill = { src: f.src, dst: d };
        showFillOptions(d);
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

  // Scroll redraws are coalesced to one per frame.
  //
  // A trackpad delivers wheel events at 100-120Hz and a thumb drag emits one
  // per mousemove, and each was calling `draw()` synchronously — so the main
  // thread was asked for two or three full repaints inside a single frame, of
  // which the browser can only ever show the last. The surplus is not just
  // wasted, it is what makes the scroll feel heavy: the handler runs long
  // enough to push the frame past its budget, so the thing being drawn arrives
  // late. Moving the offsets stays synchronous, because it is cheap and the
  // scrollbar geometry reads it; only the painting waits for the frame.
  let drawFrame = 0;
  const scheduleDraw = () => {
    if (drawFrame) return;
    drawFrame = requestAnimationFrame(() => { drawFrame = 0; draw(); });
  };

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
  // Clicking the track pages toward the click — a viewport at a time, the way
  // every other scrollbar behaves. Without it the track was inert and the only
  // way to move a long way was to drag the thumb precisely.
  const pageFromTrack = (axis, track, thumb) => (e) => {
    if (e.target !== track) return; // the thumb handles its own drag
    const v = { w: wrap.clientWidth / state.zoom, h: wrap.clientHeight / state.zoom };
    const page = axis === "v" ? Math.max(1, v.h - HH) : Math.max(1, v.w - HW);
    const r = thumb.getBoundingClientRect();
    const before = axis === "v" ? e.clientY < r.top : e.clientX < r.left;
    if (axis === "v") state.scrollY = Math.max(0, state.scrollY + (before ? -page : page));
    else state.scrollX = Math.max(0, state.scrollX + (before ? -page : page));
    clampScroll();
    draw();
  };
  vscroll.addEventListener("mousedown", pageFromTrack("v", vscroll, vthumb));
  hscroll.addEventListener("mousedown", pageFromTrack("h", hscroll, hthumb));
  window.addEventListener("mousemove", (e) => {
    if (!sbDrag) return;
    if (sbDrag.axis === "v") {
      const d = e.clientY - sbDrag.start;
      state.scrollY = Math.max(0, sbDrag.scroll0 + (d / scrollMeta.vSpan) * scrollMeta.maxScrollY);
    } else {
      const d = e.clientX - sbDrag.start;
      state.scrollX = Math.max(0, sbDrag.scroll0 + (d / scrollMeta.hSpan) * scrollMeta.maxScrollX);
    }
    scheduleDraw();
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
      // Shift turns a vertical wheel horizontal — the convention everywhere, and
      // the only way to pan sideways on a mouse with one wheel.
      if (e.shiftKey && e.deltaX === 0) {
        state.scrollX += e.deltaY * unit * scrollDamp;
      } else {
        state.scrollY += e.deltaY * unit * scrollDamp;
        state.scrollX += e.deltaX * unit * scrollDamp;
      }
      clampScroll();
      scheduleDraw();
    },
    { passive: false },
  );
  canvas.addEventListener("keydown", async (e) => {
    if (state.editing) return;
    // Alt+Down opens the active cell's validation dropdown (Excel parity).
    if (e.altKey && e.key === "ArrowDown" && validationChevron) { openValidationMenu(); e.preventDefault(); return; }
    const mod = e.ctrlKey || e.metaKey;

    // Alt+= — AutoSum. Outside the Ctrl/Cmd branch on purpose: Alt is not one
    // of those, and putting it there meant the key never fired and the "="
    // fell through to starting an edit.
    if (e.altKey && !mod && (e.key === "=" || e.key === "+")) {
      autoSum();
      e.preventDefault();
      return;
    }

    // Alt+PageUp/PageDown page sideways (Excel parity).
    //
    // **Outside the Ctrl/Cmd branch**, where it spent its life one nesting
    // level too deep: inside `if (mod)` it answered only to Ctrl+Alt+PgUp/PgDn,
    // so Excel's own binding did nothing at all and the feature was unreachable
    // by the keys it documents (`UX-PAGE-01`). It sits with the other Alt
    // shortcuts, which are out here for exactly the same reason — Alt is not
    // one of `mod`, and Alt+= had already been moved for it.
    //
    // Placed before the branch rather than after, so Ctrl+Alt keeps paging
    // sideways as it always did instead of falling through to the sheet switch.
    if (e.altKey && (e.key === "PageDown" || e.key === "PageUp")) {
      const v = wrap.clientWidth / state.zoom;
      state.scrollX += (e.key === "PageDown" ? 1 : -1) * Math.max(1, v - HW);
      clampScroll();
      draw();
      e.preventDefault();
      return;
    }

    // Keyboard shortcuts.
    if (mod) {
      // Ctrl+Arrow: jump to the data-edge (Excel block-jump).
      const arrow = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (arrow) {
        // Jump from the travelling corner when extending, so a second
        // Ctrl+Shift+Arrow carries on from where the first one reached rather
        // than measuring again from the stationary active cell.
        const from = e.shiftKey ? state.anchor : state.sel;
        const to = JSON.parse(wasm.session_edge(state.sheet, from.row, from.col, arrow[0], arrow[1]));
        if (e.shiftKey) extend(to.row, to.col); else select(to.row, to.col);
        e.preventDefault(); return;
      }
      // Ctrl+PageDown / PageUp switch sheets (Excel parity).
      if (e.key === "PageDown") { const n = JSON.parse(wasm.session_sheet_names()).length; if (state.sheet < n - 1) switchSheet(state.sheet + 1); e.preventDefault(); return; }
      if (e.key === "PageUp") { if (state.sheet > 0) switchSheet(state.sheet - 1); e.preventDefault(); return; }
      const k = e.key.toLowerCase();
      // Ctrl+T creates a table over the selection, as in Excel.
      if (k === "t") { tableDialog(); e.preventDefault(); return; }
      // Excel's Ctrl+` — show formulas instead of results.
      if (k === "`") { setViewOption("formulas"); e.preventDefault(); return; }
      // Print the sheet, not the app: the grid is a canvas, so the browser's
      // own print of this page would produce one clipped screenshot.
      if (k === "p") { printSheet(); e.preventDefault(); return; }
      // Shift extends rather than collapses. Both of these called `select`
      // unconditionally, so Ctrl+Shift+End — "select everything from here
      // down", one of the most-used keys there is — threw the selection away
      // and left a single cell. The sibling handlers a few lines up (Ctrl+arrow)
      // and below (plain Home/End) both branch on it already.
      if (k === "home") {
        if (e.shiftKey) extend(0, 0); else select(0, 0);
        e.preventDefault(); return;
      }
      if (k === "end") {
        const b = usedBounds();
        if (e.shiftKey) extend(b.rows - 1, b.cols - 1); else select(b.rows - 1, b.cols - 1);
        e.preventDefault(); return;
      }
      // Ctrl+D / Ctrl+R: fill the selection down from its top row / right from
      // its left column — the fastest way to copy a formula over a block.
      if (k === "d" && !e.shiftKey) { fillWithin("down"); e.preventDefault(); return; }
      if (k === "r" && !e.shiftKey) { fillWithin("right"); e.preventDefault(); return; }
      // Excel's number-format shortcuts. These are muscle memory for anyone who
      // uses a spreadsheet daily, and the codes are Excel's own — `0.00` for
      // Number rather than something tidier, because a file saved here has to
      // read the same as one saved there.
      if (e.shiftKey) {
        const NUMFMT = {
          "~": "General", "`": "General",
          "!": "#,##0.00", "1": "#,##0.00",
          "$": "$#,##0.00", "4": "$#,##0.00",
          "%": "0%", "5": "0%",
          "^": "0.00E+00", "6": "0.00E+00",
          "#": "d-mmm-yy", "3": "d-mmm-yy",
          "@": "h:mm AM/PM", "2": "h:mm AM/PM",
        };
        const code = NUMFMT[e.key] || NUMFMT[k];
        if (code) { setNumberFormat(code); e.preventDefault(); return; }
      }
      // Ctrl+9 hides rows and Ctrl+0 hides columns in Excel. Zoom reset moved
      // to Ctrl+Alt+0: a shortcut that does something *else* in Excel is worse
      // than one that is missing, because the finger memory is already wrong.
      if (k === "0" && e.altKey) { setZoom(1); e.preventDefault(); return; }
      if (k === "9" && !e.shiftKey) {
        const r = effectiveRange();
        tryEdit(() => wasm.session_hide_rows(state.sheet, r.r0, r.r1));
        e.preventDefault(); return;
      }
      if (k === "0" && !e.shiftKey) {
        const r = effectiveRange();
        tryEdit(() => wasm.session_hide_cols(state.sheet, r.c0, r.c1));
        e.preventDefault(); return;
      }
      // Ctrl+K — Insert Hyperlink. The menu has advertised this all along
      // without anything listening for it.
      if (k === "k") { hyperlinkDialog(); e.preventDefault(); return; }
      // Ctrl+; stamps today's date, Ctrl+Shift+; the time. Both are *static*
      // values, not TODAY()/NOW() — that is the whole point of them, and it is
      // why they need no clock in the calc engine.
      if (e.key === ";") {
        const now = new Date();
        const text = e.shiftKey
          ? `${now.getHours()}:${String(now.getMinutes()).padStart(2, "0")}`
          : `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
        tryEdit(() => wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, text));
        e.preventDefault(); return;
      }
      // Ctrl+1 — Format Cells, the shortcut every spreadsheet user already has
      // in their fingers.
      if (k === "1" && !e.shiftKey) { formatCellsDialog(); e.preventDefault(); return; }
      // Ctrl+Alt+V — Paste Special, as everywhere else.
      if (k === "v" && e.altKey) { pasteSpecialDialog(); e.preventDefault(); return; }
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
      // Ctrl+V is *not* handled here. Letting the browser raise its own
      // `paste` event is what puts `clipboardData` in our hands, and that is
      // the only way to read the `text/html` flavour without asking for
      // clipboard-read permission. The listener below does the work.
      if (k === "v") { return; }
      // Ctrl+Shift+"+" inserts rows/columns, Ctrl+"-" deletes them (Excel).
      // "+" arrives as key "+" or as "=" with Shift depending on the layout.
      if ((e.key === "+" || (k === "=" && e.shiftKey))) { insertLines(); e.preventDefault(); return; }
      if (e.key === "-" || e.key === "_") { deleteLines(); e.preventDefault(); return; }
    }

    // A Shift-step continues from the corner that is travelling — which is
    // `state.anchor` (see `extend`) — while a plain step moves the active cell.
    // After a plain `select` the two are the same cell, so this is only ever
    // different mid-extension, which is exactly when it matters.
    const move = (dr, dc) => {
      if (e.shiftKey) {
        const to = stepFrom(state.anchor.row, state.anchor.col, dr, dc);
        extend(to.row, to.col);
      } else {
        const to = stepFrom(state.sel.row, state.sel.col, dr, dc);
        select(to.row, to.col);
      }
    };
    // **End mode** (Excel): `End` on its own arms it and moves nothing; the next
    // arrow jumps to the edge of the data block, and `End` then `Home` goes to
    // the last used cell. It is how a keyboard user crosses a large sheet
    // without holding a modifier, and it was the one navigation idiom missing
    // (`UX-END-01`).
    //
    // Handled before the switch below, because armed `End` changes what the
    // *next* key means — and because arming it must not also move the cursor,
    // which is what `End` used to do.
    if (state.endMode && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const jump = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (jump) {
        setEndMode(false);
        const from = e.shiftKey ? state.anchor : state.sel;
        const to = JSON.parse(wasm.session_edge(state.sheet, from.row, from.col, jump[0], jump[1]));
        if (e.shiftKey) extend(to.row, to.col); else select(to.row, to.col);
        e.preventDefault();
        return;
      }
      if (e.key === "Home") {
        // Excel's End,Home — the bottom-right of the used range.
        setEndMode(false);
        const b = usedBounds();
        const at = { r: Math.max(0, b.rows - 1), c: Math.max(0, b.cols - 1) };
        if (e.shiftKey) extend(at.r, at.c); else select(at.r, at.c);
        e.preventDefault();
        return;
      }
      // Anything else cancels it, as in Excel — an armed mode that survives an
      // unrelated keystroke fires on a later arrow the user has forgotten about.
      if (e.key !== "End" && e.key !== "Shift") setEndMode(false);
    }
    switch (e.key) {
      case "ArrowUp": move(-1, 0); e.preventDefault(); break;
      case "ArrowDown": move(1, 0); e.preventDefault(); break;
      case "Enter": enterStep(e.shiftKey); e.preventDefault(); break;
      case "ArrowLeft": move(0, -1); e.preventDefault(); break;
      case "ArrowRight": move(0, 1); e.preventDefault(); break;
      case "Tab": tabStep(e.shiftKey); e.preventDefault(); break;
      case "Home": if (e.shiftKey) extend(state.anchor.row, 0); else select(state.sel.row, 0); e.preventDefault(); break;
      // **`End` alone arms End mode and moves nothing**, which is what Excel
      // does. It used to jump to the last used column — which is Excel's
      // *`End` then `Right`*, so the shortcut existed while the mode it belongs
      // to did not, and the two-key idiom a spreadsheet user has in their
      // fingers did nothing.
      case "End": setEndMode(!state.endMode); e.preventDefault(); break;
      case "PageDown": { const p = Math.max(1, geo.rows - 1); move(p, 0); e.preventDefault(); break; }
      case "PageUp": { const p = Math.max(1, geo.rows - 1); move(-p, 0); e.preventDefault(); break; }
      case "Backspace": case "Delete":
        // A selected chart is what Delete acts on, as in Excel — the cells
        // under it are not selected and must not be cleared instead.
        if (chartSel && chartSel.sheet === state.sheet) {
          tryEdit(() => wasm.session_delete_chart(chartSel.sheet, chartSel.index));
          chartSel = null;
          panelChart = null;
          if (activePanel === "chart") refreshChartPanel();
          status.textContent = "chart deleted";
        } else {
          clearSelection();
        }
        e.preventDefault();
        break;
      case "F2": {
        if (e.shiftKey) openPanel("note"); // Shift+F2 → note (Excel parity)
        else startInline(undefined, true);
        e.preventDefault(); break;
      }
      // Plain F5 is Go To (the name box). Alt+F5 refreshes the pivot under the
      // cursor and Ctrl+Alt+F5 refreshes every one, both Excel's bindings.
      case "F5":
        if (e.altKey && (e.ctrlKey || e.metaKey)) refreshAllPivots();
        else if (e.altKey) refreshPivotHere();
        else cellRef.focus();
        e.preventDefault();
        break;
      // F9 recalculates, F11 (Shift) adds a sheet — both Excel's.
      case "F9":
        recalculateNow();
        e.preventDefault();
        break;
      case "F11":
        if (e.shiftKey) {
          try { switchSheet(wasm.session_add_sheet()); renderTabs(); }
          catch (err) { statusError(errText(err)); }
          e.preventDefault();
        }
        break;
      case " ": if (e.shiftKey) selectRowsSpan(); else startInline(" "); e.preventDefault(); break; // Shift+Space → whole rows
      case "Escape":
        if (painter) { setPainter(null); status.textContent = "format painter off"; e.preventDefault(); }
        else if (clipMarch) { stopMarch(); e.preventDefault(); }
        else if (chartSel) { chartSel = null; draw(); e.preventDefault(); }
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
      // Excel's "enter mode": while *typing a fresh value*, an arrow commits and
      // moves. It must not while amending an existing value (the arrows move the
      // caret) or mid-formula (they pick references), which is what `editMode`
      // and the leading `=` distinguish.
      else if (
        editMode === "Enter" &&
        !surface.value.trim().startsWith("=") &&
        ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)
      ) {
        if (commit(surface.value, false)) {
          const d = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
          select(state.sel.row + d[0], state.sel.col + d[1]);
        }
        e.preventDefault();
      }
      else if (e.key === "Tab") {
        // Shift+Tab while editing moves left, which it previously could not.
        if (commit(surface.value, false)) tabStep(e.shiftKey);
        e.preventDefault();
      }
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
  byId("find-next").addEventListener("click", () => findStep(1));
  byId("find-prev").addEventListener("click", () => findStep(-1));
  byId("replace-one").addEventListener("click", replaceOne);
  byId("replace-all").addEventListener("click", replaceAll);
  byId("find-close").addEventListener("click", closeFind);

  byId("hdr-open").addEventListener("click", () => byId("tb-open").click());

  // Popover menus: click toggles, outside-click / Escape closes, only one open.
  const menus = [];
  // The menus are `position: fixed` (to escape the toolbar's overflow clip), so
  // anchor each under its trigger button in viewport coordinates, flipping to
  // stay on-screen at the right and bottom edges.
  function anchorMenu(menu, btn) {
    const r = btn.getBoundingClientRect();
    menu.style.left = "0px";
    menu.style.top = "0px";
    menu.style.maxHeight = "";
    const mw = menu.offsetWidth, mh = menu.offsetHeight;
    let left = r.left;
    if (left + mw > window.innerWidth - 4) left = Math.max(4, window.innerWidth - 4 - mw);

    // Below the button if it fits, above if that fits better, and **clamped**
    // if neither does.
    //
    // It used to flip up unconditionally and clamp the result to 4px, which put
    // the tallest menu — Format — at the very top of the page, nowhere near the
    // button that opened it. Embedding made it routine rather than rare: an
    // editor half-way down a page has little room below and a menu taller than
    // the space above it.
    const below = window.innerHeight - 4 - (r.bottom + 4);
    const above = r.top - 8;
    let top;
    if (mh <= below || below >= above) {
      top = r.bottom + 4;
      if (mh > below) menu.style.maxHeight = Math.max(120, below) + "px";
    } else {
      top = Math.max(4, r.top - 4 - Math.min(mh, above));
      if (mh > above) menu.style.maxHeight = Math.max(120, above) + "px";
    }
    menu.style.left = left + "px";
    menu.style.top = top + "px";
  }
  function wirePopup(btnId, menuId, onItem) {
    const btn = byId(btnId);
    const menu = byId(menuId);
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
    const btn = byId(btnId);
    const menu = byId(menuId);
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
    const menu = byId("border-menu");
    for (const sw of menu.querySelectorAll(".bd-color")) {
      sw.classList.toggle("on", (sw.dataset.color || "") === borderColor);
    }
    const sel = menu.querySelector(".bd-style");
    if (sel) sel.value = borderStyle;
    const btn = byId("tb-border");
    btn.style.setProperty("--oc-x-border-swatch", borderColor ? "#" + borderColor : "currentColor");
  }
  {
    const btn = byId("tb-border");
    const menu = byId("border-menu");
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
  byId("tb-filter").addEventListener("click", (e) => {
    e.stopPropagation();
    toggleFilter();
  });
  // Tool side panel: toolbar buttons toggle it; the header ✕ and Esc close it.
  byId("tb-dv").addEventListener("click", () => togglePanel("dv"));
  byId("tb-cf").addEventListener("click", () => togglePanel("cf"));
  byId("tb-note").addEventListener("click", () => togglePanel("note"));
  byId("side-panel-close").addEventListener("click", () => closePanel());

  byId("tb-size-up").addEventListener("click", () => { stepFontSize(1); canvas.focus(); });
  byId("tb-size-down").addEventListener("click", () => { stepFontSize(-1); canvas.focus(); });
  byId("tb-bold").addEventListener("click", () => { toggleBold(); canvas.focus(); });
  byId("tb-italic").addEventListener("click", () => { toggleItalic(); canvas.focus(); });
  byId("tb-underline").addEventListener("click", () => { toggleUnderline(); canvas.focus(); });
  byId("tb-strike").addEventListener("click", () => { toggleStrike(); canvas.focus(); });
  byId("tb-indent-more").addEventListener("click", () => { setIndent(1); canvas.focus(); });
  byId("tb-indent-less").addEventListener("click", () => { setIndent(-1); canvas.focus(); });
  {
    const pb = byId("tb-painter");
    pb.addEventListener("click", () => { painter ? setPainter(null) : armPainter(false); canvas.focus(); });
    pb.addEventListener("dblclick", () => { armPainter(true); canvas.focus(); });
  }
  byId("tb-currency").addEventListener("click", () => { setNumberFormat("$#,##0.00"); canvas.focus(); });
  // These are toggles, not one-way switches: pressing the button that is already
  // applied returns the cell to General, which is the only way back without
  // hunting through the number menu.
  const toggleFormat = (code) => () => {
    let current = "";
    try { current = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col)).nf || ""; }
    catch {}
    setNumberFormat(current === code ? "" : code);
    canvas.focus();
  };
  byId("tb-percent").addEventListener("click", toggleFormat("0%"));
  byId("tb-comma").addEventListener("click", toggleFormat("#,##0.00"));
  byId("tb-inc-dec").addEventListener("click", () => { adjustDecimals(1); canvas.focus(); });
  byId("tb-dec-dec").addEventListener("click", () => { adjustDecimals(-1); canvas.focus(); });
  for (const b of qsa(".tb-align")) {
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
    input: byId("tb-font"),
    caret: byId("tb-font-caret"),
    menu: byId("font-menu"),
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
    input: byId("tb-size"),
    caret: byId("tb-size-caret"),
    menu: byId("size-menu"),
    values: [{ v: "", label: "Default", title: "Clear the size (use the workbook default)" }]
      .concat(SIZE_LADDER.map((n) => ({ v: String(n), label: String(n) }))), // same ladder as A▲/A▼
    apply: (v) => {
      const raw = parseFloat(v);
      setFontSize(Number.isFinite(raw) && raw > 0 ? Math.min(409, Math.max(1, raw)) : 0);
    },
  });
  // What the picker offers is the engine's list, not the markup's. The static
  // one in `editor.html` is a second table of supported formats and drifted
  // from the first — it omitted `.tab`, which the engine has always read — and
  // a format the engine grows would be unpickable until somebody remembered to
  // edit the HTML too (`WASM-01`).
  try {
    const offered = JSON.parse(wasm.openable_extensions());
    if (offered.length) {
      byId("tb-open").accept = offered.join(",");
      // The tooltip was a third copy of the same list.
      byId("hdr-open")?.setAttribute("title", `Open a file (${offered.join(", ")})`);
    }
  } catch {}
  byId("tb-open").addEventListener("change", async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    // A large file takes a moment to parse; say so rather than appearing to
    // have ignored the click.
    status.textContent = `opening ${file.name}…`;
    // A macrotask, not `requestAnimationFrame`: rAF does not fire in a
    // backgrounded tab, so waiting on it here hung the open entirely whenever
    // the window was not in front.
    await new Promise((r) => setTimeout(r, 0));
    const ok = openBytes(new Uint8Array(await file.arrayBuffer()), file.name);
    e.target.value = ""; // allow re-opening the same file
    return ok;
  });
  byId("fx-insert").addEventListener("click", (e) => {
    e.stopPropagation();
    insertFunctionDialog();
  });
  byId("fx-expand").addEventListener("click", (e) => {
    e.stopPropagation();
    // A long formula is unreadable in a one-line box; expanding gives it room
    // without opening a dialog that would lose the caret.
    const bar = qs(".formula-bar");
    const on = bar.classList.toggle("expanded");
    e.currentTarget.setAttribute("aria-expanded", on ? "true" : "false");
    resize();
  });
  byId("name-box-list").addEventListener("click", (e) => {
    e.stopPropagation();
    openNameBoxList();
  });
  byId("tb-undo").addEventListener("click", doUndo);
  byId("tb-redo").addEventListener("click", doRedo);

  // --- Progressive toolbar collapse (Excel-ribbon style) ---
  // Each group tagged data-collapse=<priority> collapses in-place into its
  // "Label ▾" button — whose flyout holds the group's live tools — whenever the
  // single toolbar row would overflow, lowest priority first. Groups re-expand
  // as the window widens. Never a scrollbar, never a second row.
  const toolbarEl = qs(".toolbar");
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
    const toolbar = qs(".toolbar");
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
      const focused = list.findIndex((el) => el === activeEl());
      list[Math.max(0, focused)].tabIndex = 0;
    };
    syncStops();
    toolbar.addEventListener("focusin", syncStops);
    toolbar.addEventListener("keydown", (e) => {
      // A text field owns its own arrow keys (the font and size boxes), so only
      // step between controls when the caret is not in one.
      const inField = activeEl() && activeEl().tagName === "INPUT";
      if (inField && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;
      const list = items();
      const at = list.indexOf(activeEl());
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
    if (e.key === "Tab" || e.key.startsWith("Arrow")) ocThemeHost.dataset.kbnav = "1";
  }, true);
  window.addEventListener("mousedown", () => { delete ocThemeHost.dataset.kbnav; }, true);

  buildMenuBar();

  // The page-header collapse toggle, kept right-most: buildMenuBar() appends
  // File…Help, so re-append it afterwards rather than relying on markup order.
  {
    const btn = byId("hdr-collapse");
    const bar = byId("menubar");
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
    const bar = byId("menubar");
    if (!bar) return;
    const q = (sel) => qs(sel);
    const clickEl = (sel) => () => { const n = q(sel); if (n) n.click(); };
    const rng = () => effectiveRange();
    const fmtHas = (k) => {
      try { return !!JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col))[k]; }
      catch { return false; }
    };
    const gridOn = () => { try { return !wasm.session_gridlines_hidden(state.sheet); } catch { return true; } };
    const viewOn = (k) => { try { return !!viewOptions()[k]; } catch { return false; } };
    const cellProt = (k) => {
      try {
        return !!JSON.parse(
          wasm.session_cell_protection(state.sheet, state.sel.row, state.sel.col))[k];
      } catch { return false; }
    };
    // The active cell's number format and alignment, for the submenu ticks — a
    // menu that never shows what is already applied makes you guess.
    const curFmt = (key) => {
      try { return JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col))[key] || ""; }
      catch { return ""; }
    };
    const nfIs = (code) => () => curFmt("nf") === code;
    const alIs = (token) => () => curFmt("al") === token;
    const vaIs = (token) => () => curFmt("va") === token;
    const vtIs = (token) => () => curFmt("vt") === token;
    const headersOn = () => { try { return !wasm.session_headers_hidden(state.sheet); } catch { return true; } };
    // `clearSelection` rather than a second copy of it: the copy had its own
    // swallowing catch, so the same command reported a refusal from the Delete
    // key and said nothing from the menu.
    const clearContents = () => clearSelection();
    const nf = (code) => () => setNumberFormat(code);

    const showModal = (title, html) => {
      byId("oc-modal-title").textContent = title;
      // oc-safe-html: both callers pass literal markup built in this file
      // (the shortcut table and the About text). It must stay that way —
      // this helper is not for anything a document can influence.
      // oc-safe-html: see the note above.
      byId("oc-modal-body").innerHTML = html;
      byId("oc-modal").hidden = false;
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
      showModal(`About ${BRAND}`,
        `<p>${htmlText(BRAND)} — a deterministic, embeddable spreadsheet engine for <code>.xlsx</code>, CSV, TSV and PSV.</p>
         <p style="margin-top:10px;color:var(--oc-muted-text-color)">Engine <b>v0.0.0</b> · Alpha · <a href="./index.html">Home</a></p>`);
    }

    const MENUS = [
      ["File", [
        ["New", () => { stopMarch(); wasm.session_new(); imageCache.clear(); state.sheet = 0; seed(); renderTabs(); }],
        ["Open…", clickEl("#tb-open")],
        { sub: "Download", items: [
          // First, and named for what it does rather than for a format: it is
          // the only one of these that gives back the kind of file that was
          // opened. The others are conversions, and a conversion chosen by
          // accident is how a `.csv` becomes a package under its own name.
          ["Same format as opened", () => saveAs("native")],
          ["Excel (.xlsx)", () => saveAs("xlsx")],
          ["CSV (.csv)", () => saveAs("csv")],
          ["Tab-separated (.tsv)", () => saveAs("tsv")],
          ["Pipe-separated (.psv)", () => saveAs("psv")],
        ] },
        "sep",
        ["Page setup…", () => openPanel("page")],
        // Excel inserts both a row and a column break at the active cell, and
        // only the one that applies for a whole-row/column selection.
        ["Page break here", () => {
          const r = effectiveRange();
          const rowAt = state.selKind === "cols" ? 0xffffffff : r.r0;
          const colAt = state.selKind === "rows" ? 0xffffffff : r.c0;
          tryEdit(() => wasm.session_toggle_page_break(state.sheet, rowAt, colAt));
        }],
        ["Print…", () => printSheet(), "Ctrl+P"],
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
        // Fill from the top/left of the selection into the rest of it. The fill
        // handle can do the same, but only by dragging — and a mode you can only
        // reach by dragging is one most people never find.
        { sub: "Fill", items: [
          ["Fill down", () => fillWithin("down"), "Ctrl+D"],
          ["Fill right", () => fillWithin("right"), "Ctrl+R"],
          "sep",
          ["Fill series", () => fillSelection("series")],
          ["Growth series", () => fillSelection("growth")],
          ["Copy cells", () => fillSelection("copy")],
          ["Formatting only", () => fillSelection("formats")],
          ["Without formatting", () => fillSelection("values")],
        ] },
      ]],
      ["View", [
        { sub: "Freeze", items: [
          ["Up to selection", clickEl('#freeze-menu [data-fz="sel"]')],
          ["Top row", clickEl('#freeze-menu [data-fz="row"]')],
          ["First column", clickEl('#freeze-menu [data-fz="col"]')],
          ["Unfreeze", clickEl('#freeze-menu [data-fz="none"]')],
        ] },
        // Both toggles report a refusal. Swallowing it left the tick unmoved
        // with no reason given, which reads as a menu item that is broken.
        ["Gridlines", () => {
          try { wasm.session_set_gridlines_hidden(state.sheet, gridOn()); }
          catch (err) { statusError(errText(err)); }
          draw();
        }, null, gridOn],
        // "Cell markings" = the A/B/C and 1/2/3 strips. Deliberately not called
        // "headers": that word belongs to the page header this menu bar can
        // collapse, and having both under one name is a coin-flip every time.
        ["Cell markings", () => {
          try { wasm.session_set_headers_hidden(state.sheet, headersOn()); }
          catch (err) { statusError(errText(err)); }
          resize();
        }, null, headersOn],
        // Both are per-sheet OOXML view flags that were being carried through
        // every save without ever being shown.
        ["Formulas instead of results", () => setViewOption("formulas"), "Ctrl+`", () => viewOn("formulas")],
        ["Zero values", () => setViewOption("zeros"), null, () => viewOn("zeros")],
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
        "sep",
        // The context menu and Ctrl+T both reach this, but neither is somewhere
        // you look for a feature you have not met yet.
        ["Table…", () => tableDialog(), "Ctrl+T"],
        ["PivotTable…", () => pivotDialog()],
        { sub: "Chart", items: CHART_KINDS.map(([kind, label]) => [label, () => chartDialog(kind)]) },
        ["Hyperlink…", () => hyperlinkDialog(), "Ctrl+K"],
      ]],
      ["Format", [
        ["Bold", clickEl("#tb-bold"), "Ctrl+B", () => fmtHas("b")],
        ["Italic", clickEl("#tb-italic"), "Ctrl+I", () => fmtHas("i")],
        ["Underline", clickEl("#tb-underline"), "Ctrl+U", () => fmtHas("u")],
        ["Strikethrough", clickEl("#tb-strike"), null, () => fmtHas("st")],
        // Both are one `vertAlign` on the font, so they are mutually exclusive:
        // choosing one replaces the other rather than stacking.
        ["Superscript", () => setVertAlign("superscript"), null, vtIs("superscript")],
        ["Subscript", () => setVertAlign("subscript"), null, vtIs("subscript")],
        "sep",
        { sub: "Protection", items: [
          // Both only bite while the sheet is protected, so the sheet switch
          // sits in the same menu rather than somewhere else entirely.
          ["Locked", () => setCellProtection("locked"), null, () => cellProt("locked")],
          ["Hide formula", () => setCellProtection("hidden"), null, () => cellProt("hidden")],
          "sep",
          ["Protect this sheet", () => toggleSheetProtected(), null, sheetProtectedNow],
        ] },
        ["Cell styles…", () => cellStyleGallery()],
        ["Conditional formatting rules…", () => manageCfRules()],
        { sub: "Trace", items: [
          ["Trace precedents", () => toggleTrace("prec")],
          ["Trace dependents", () => toggleTrace("dep")],
          ["Clear trace arrows", () => clearTrace()],
        ] },
        { sub: "Alignment", items: [
          ["Left", () => setAlign("left"), null, alIs("left")],
          ["Center", () => setAlign("center"), null, alIs("center")],
          ["Right", () => setAlign("right"), null, alIs("right")],
          // The OOXML modes that are more than an edge. `centerContinuous` is
          // Excel's "Center Across Selection" — it looks merged but merges
          // nothing, so the cells underneath stay addressable.
          ["Fill (repeat text)", () => setAlign("fill")],
          ["Justify", () => setAlign("justify")],
          ["Center across selection", () => setAlign("centerContinuous")],
          ["Distributed", () => setAlign("distributed")],
          ["Clear (General)", () => setAlign("")],
          "sep",
          ["Top", () => setValign("top"), null, vaIs("t")],
          ["Middle", () => setValign("middle"), null, vaIs("m")],
          ["Bottom", () => setValign("bottom"), null, vaIs("b")],
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
          ["Automatic", nf(""), null, nfIs("")],
          ["Number (0.00)", nf("0.00"), null, nfIs("0.00")],
          ["Thousands (#,##0)", nf("#,##0"), null, nfIs("#,##0")],
          ["Percent (0%)", nf("0%"), null, nfIs("0%")],
          ["Currency", nf("$#,##0.00"), null, nfIs("$#,##0.00")],
          ["Short date", nf("yyyy-mm-dd"), null, nfIs("yyyy-mm-dd")],
          ["Time", nf("h:mm:ss AM/PM"), null, nfIs("h:mm:ss AM/PM")],
          ["Scientific", nf("0.00E+00"), null, nfIs("0.00E+00")],
          ["Text", nf("@"), null, nfIs("@")],
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
        ["Text to columns…", () => textToColumnsDialog()],
        ["Filter", () => toggleFilter()],
        ["Clear all filters", () => { if (!filterInfo) { status.textContent = "no filter"; return; } tryEdit(() => wasm.session_clear_filter_rules(state.sheet)); afterFilterChange(); }],
        ["Clear my view", () => clearMyView()],
        ["Data validation…", clickEl("#tb-dv")],
        "sep",
        ["PivotTable fields…", () => pivotDialog()],
        ["Refresh pivot", () => refreshPivotHere(), "Alt+F5"],
        ["Refresh all pivots", () => refreshAllPivots(), "Ctrl+Alt+F5"],
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
        "sep",
        // Excel's Formulas ▸ Calculation Options. A workbook saved with
        // calculation off opens that way, so the state has to be visible and
        // changeable rather than assumed.
        { sub: "Calculation", items: [
          ["Automatic", () => setCalculationMode("auto"), null, () => calcMode() === "auto"],
          ["Manual", () => setCalculationMode("manual"), null, () => calcMode() === "manual"],
          "sep",
          ["Calculate now", () => recalculateNow(), "F9"],
        ] },
      ]],
      ["Help", [
        ["Keyboard shortcuts", showShortcuts],
        [`About ${BRAND}`, showAbout],
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
    // Tick every item under `root` that has a predicate.
    //
    // Called per menu *and* per submenu: a submenu is appended to the body, not
    // to its parent dropdown, so refreshing only the dropdown left every nested
    // item — Alignment, Number, Zoom, Text overflow, Protection — permanently
    // unticked whatever the cell actually had.
    const refreshChecks = (root) => {
      for (const b of root.querySelectorAll("button")) {
        if (b._check) b.querySelector(".mi-check").textContent = b._check() ? "✓" : "";
      }
    };
    const runItem = (action) => {
      try { action && action(); } catch (err) { statusError(errText(err)); }
      closeMenus();
    };
    const renderItems = (container, items, isTop, path = "") => {
      for (const it of items) {
        if (it === "sep") {
          const s = document.createElement("div"); s.className = "menu-sep"; container.appendChild(s); continue;
        }
        if (it.sub) {
          const b = document.createElement("button");
          // oc-safe-html: empty scaffolding; the label is set with textContent below.
          b.innerHTML = `<span class="mi-check"></span><span class="mi-label"></span><span class="mi-caret">&#9656;</span>`;
          b.dataset.ocLabel = it.sub;
          b.querySelector(".mi-label").textContent =
            t(`command.${commandId(path, it.sub)}`, it.sub);
          const sub = document.createElement("div"); sub.className = "menu-sub popmenu"; sub.hidden = true;
          ocOverlayHost.appendChild(sub); subs.push(sub);
          b.dataset.ocCommand = commandId(path, it.sub);
          sub.dataset.ocFor = commandId(path, it.sub);
          renderItems(sub, it.items, false, commandId(path, it.sub));
          const openSub = () => { closeSubs(); refreshChecks(sub); positionSub(sub, b); sub.hidden = false; };
          b.addEventListener("mouseenter", openSub);
          b.addEventListener("click", (e) => { e.stopPropagation(); openSub(); });
          container.appendChild(b); continue;
        }
        const [label, action, key, check] = it;
        const b = document.createElement("button");
        // oc-safe-html: scaffolding plus a shortcut label from the static
        // command table; the menu label itself is set with textContent.
        // oc-safe-html: see the note above.
        b.innerHTML = `<span class="mi-check"></span><span class="mi-label"></span>${key ? `<span class="mi-key">${key}</span>` : ""}`;
        b.dataset.ocCommand = commandId(path, label);
        b.dataset.ocLabel = label;
        b.querySelector(".mi-label").textContent = t(`command.${b.dataset.ocCommand}`, label);
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
      refreshChecks(drop); // against the focus cell / view state
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
    MENUS.forEach(([name, items], i) => {
      const btn = document.createElement("button");
      btn.className = "menu-top";
      btn.dataset.ocLabel = name;
      btn.dataset.ocMenuIndex = String(i);
      btn.setAttribute("role", "menuitem");
      // Roving tabindex: the bar is one tab stop, not nine — Tab moves past it,
      // arrows move within it, which is what a menubar is supposed to do.
      btn.tabIndex = i === 0 ? 0 : -1;
      btn.setAttribute("aria-haspopup", "true"); btn.setAttribute("aria-expanded", "false");
      const drop = document.createElement("div"); drop.className = "menu-drop popmenu"; drop.hidden = true;
      drop.setAttribute("role", "menu");
      ocOverlayHost.appendChild(drop);
      btn.dataset.ocCommand = commandId("", name);
      drop.dataset.ocFor = commandId("", name);
      renderItems(drop, items, true, commandId("", name));
      btn.addEventListener("click", (e) => { e.stopPropagation(); openIdx === i ? closeMenus() : openMenu(i); });
      btn.addEventListener("mouseenter", () => { if (openIdx >= 0 && openIdx !== i) openMenu(i); });
      bar.appendChild(btn); topBtns.push(btn); drops.push(drop);
    });
    // Labels and mnemonics together, because the second is derived from the
    // first: translating a menu changes which letters are free.
    relabelMenubar();

    // Alt+letter opens the matching menu; holding Alt alone reveals which letter
    // each menu answers to.
    document.addEventListener("keydown", (e) => {
      if (e.key === "Alt") { bar.classList.add("show-mnemonics"); return; }
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      const i = menuMnemonics.get((e.key || "").toLowerCase());
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
      const i = topBtns.indexOf(activeEl());
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
      const at = list.indexOf(activeEl());
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
    const modal = byId("oc-modal");
    byId("oc-modal-x").addEventListener("click", () => { modal.hidden = true; });
    modal.addEventListener("click", (e) => { if (e.target === modal) modal.hidden = true; });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") modal.hidden = true; });
  }
  // Esc closes the tool panel (when no context menu is open and not editing).
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && activePanel && !state.editing && !byId("sheet-ctx")) {
      closePanel();
    }
  });
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { readColors(); draw(); });

  wirePresence();
  wireSettings();
}

function applyTheme(theme) {
  if (theme === "auto") delete ocThemeHost.dataset.theme;
  else ocThemeHost.dataset.theme = theme;
  localStorage.setItem("oc-theme", theme);
  readColors();
  draw();
}

function applyAccent(color) {
  ocThemeHost.style.setProperty("--oc-accent-color", color);
  localStorage.setItem("oc-accent", color);
  for (const b of qsa("#set-accent button")) {
    b.setAttribute("aria-current", b.dataset.c === color ? "true" : "false");
  }
  readColors();
  draw();
}

function wireSettings() {
  const gear = byId("tb-settings");
  const panel = byId("settings-panel");
  const themeSel = byId("set-theme");

  gear.addEventListener("click", (e) => {
    e.stopPropagation();
    panel.hidden = !panel.hidden;
  });
  document.addEventListener("click", (e) => {
    if (!panel.contains(e.target) && e.target !== gear) panel.hidden = true;
  });
  themeSel.addEventListener("change", () => applyTheme(themeSel.value));
  for (const b of qsa("#set-accent button")) {
    b.addEventListener("click", () => applyAccent(b.dataset.c));
  }

  const scroll = byId("set-scroll");
  const scrollVal = byId("set-scroll-val");
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
  // The seeded sheet is the document, not something the user did to it.
  // Without this, holding Ctrl+Z takes the demo apart cell by cell and leaves
  // an empty grid — and Undo starts out enabled on a document nobody has
  // touched, which is its own small lie.
  wasm.session_clear_history();
  select(0, 0);
}


// Resolve the markup's elements against the mount root.
//
// Deferred rather than resolved at import time: an embedded editor sets its
// mount root *after* the module loads, and a binding taken against `document`
// before that points at nothing — or, worse, at the same id somewhere in the
// host's page.
function bindElements() {
  canvas = byId("grid");
  wrap = byId("grid-wrap");
  inline = byId("inline-edit");
  selStats = byId("sel-stats");
  vscroll = byId("vscroll");
  vthumb = byId("vthumb");
  hscroll = byId("hscroll");
  hthumb = byId("hthumb");
  fInput = byId("formula-input");
  cellRef = byId("cell-ref");
  commentTip = byId("comment-tip");
  status = byId("tb-status");
  findBar = byId("find-bar");
  findInput = byId("find-input");
  replaceInput = byId("replace-input");
  findCount = byId("find-count");
  findCase = byId("find-case");
  findWhole = byId("find-whole");
  findValues = byId("find-values");
  findAllSheets = byId("find-all-sheets");
  findWildcards = byId("find-wildcards");
  liveEl = byId("grid-live");
  modeEl = byId("cell-mode");
  a11yEl = byId("grid-a11y");
  acEl = byId("ac-menu");
  sigEl = byId("sig-tip");
  tabsEl = byId("sheet-tabs");
  ctx = canvas.getContext("2d");
}


/// Load a workbook from bytes, whatever the host got them from.
///
/// Extracted from the file-input handler so an embedded editor opens a
/// workbook by the same path a picked file does — two implementations of
/// "open" is two places for the post-load bookkeeping below to be forgotten.
///
/// `budgetMs` is how long the engine may hold the thread before the open is
/// stopped; a negative one means "no limit", which is what "Keep waiting"
/// retries with.
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
  return ok;
}

/// Re-read the theme tokens and repaint.
///
/// The canvas caches them: it paints thousands of cells a frame and cannot ask
/// for a computed style per cell. So a host changing `--oc-accent-color` restyles the
/// chrome instantly and would leave the grid on the old colours until something
/// else forced a repaint — this is what closes that gap.
export function refreshTheme() {
  readColors();
  invalidateGrowth();
  draw();
}

/// Scroll to the top-left and select A1.
///
/// What a thumbnail means: a preview scrolled to row 14 of someone else's
/// workbook is not a preview of it.
export function resetToOrigin() {
  resetView();
}

/// Re-measure and repaint after the element's box changed.
///
/// Hiding a chrome region gives the grid more room, and the canvas is sized in
/// device pixels from a measured box — so without this the grid keeps the old
/// height and leaves a gap where the toolbar used to be.
export function relayout() {
  invalidateGrowth();
  resize();
}

/// Autofit a column, for the browser gate. See [`autofitRowForTest`].
export function autofitColumnForTest(col) {
  autofitColumn(col);
}

/// Autofit a row, for the browser gate.
///
/// Exported because the alternative was asserting nothing about autofit: it is
/// reached by double-clicking a row boundary, which a test can only simulate by
/// guessing at pixel coordinates on a canvas — and a test that guesses wrong
/// passes for the wrong reason.
export function autofitRowForTest(row) {
  autofitRow(row);
}

/// Where the body is scrolled to, and where the selection is, for the browser
/// gate.
///
/// Exported because the property worth asserting — that the leading edge sits on
/// a whole column and a whole row — is invisible from outside: the grid is a
/// canvas, so a test can only see it by sampling pixels and deciding what a
/// column boundary looks like, which is a test of the sampling. The number the
/// renderer actually uses is the claim.
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

/// Set the magnification, for the browser gate.
///
/// Routed through the editor's own `setZoom` rather than assigning
/// `state.zoom`, so the gate exercises what the menu does — including whatever
/// clamping and repaint that entails.
export function setZoomForTest(z) {
  setZoom(z);
}

/// Move the selection, for the browser gate — the keyboard path, without the
/// keyboard.
///
/// `ensureVisible` is what decides the scroll offset, and it is reached by
/// arrowing off the edge of the viewport. Synthesising that needs the canvas
/// focused and the right number of key events for the window size, which is a
/// test that measures the harness.
/// Routed through the editor's own `select`, not a copy of it: a test that
/// reimplements the path it is testing agrees with itself.
export function selectForTest(row, col) {
  select(row, col);
}

/// The selected rectangle, for the browser gate.
///
/// `scrollStateForTest` reports the *active cell*, which is all a step needs;
/// an extend has to be asserted on the rectangle, because the bug it guards
/// against is precisely a selection collapsing to one cell while the active cell
/// looks right.
export function selectionRectForTest() {
  return effectiveRange();
}

/// The raw engine bindings, for a host that needs something the element does
/// not wrap. Deliberately the same object the editor itself uses: a second
/// session would be a second workbook.
export function wasmApi() {
  return wasm;
}

// --- Collaboration ----------------------------------------------------------
//
// The transport lives in `collab.js` and knows nothing about a grid. This is
// the other half: what the editor does when a document arrives, when somebody
// else's edit lands, and when a participant moves.
//
// The join is *here*, rather than left to the host, for one concrete reason.
// The engine is a module-scope binding imported under a cache-busting
// specifier, and each mounted element imports its own copy — so a host that
// imported the glue itself would get a second, uninitialised instance and every
// call would throw from inside the generated bindings. Handing the host a
// `collaborate()` that closes over *this* editor's engine removes a decision
// nobody can make correctly from outside.

/// The live session, or null.
let collabSession = null;

/// Everyone else, by client id. What draws a remote cursor, and what a host
/// reads to render a participant list.
const collabRoster = new Map();

/// What was last announced — sheet, selection and draft — so presence is sent
/// when it changes and not on every frame. `draw()` polls this the way
/// `emitStateEvents` does, for the same reason: there are dozens of places the
/// selection changes and one of them will always be forgotten.
let collabAnnounced = "";

/// The floor between two presence messages, and the trailing timer that makes
/// the floor safe.
///
/// A draft changes on every keystroke, and a message per keystroke is a message
/// per keystroke for every other participant to receive, parse and repaint. A
/// touch-typist runs at about eight a second, so this collapses a burst into
/// roughly six a second while leaving the *first* keystroke immediate — a
/// leading edge, because the interesting moment for everyone else is the one
/// where somebody starts typing in a cell.
///
/// The trailing timer is not optional. Throttling by dropping would drop the
/// **last** keystroke of a burst, which is the one that stays on screen: peers
/// would sit looking at "=SUM(A1:A9" for as long as the author admired their
/// finished formula.
const PRESENCE_THROTTLE_MS = 150;
let collabAnnounceTimer = null;
let collabAnnouncedAt = 0;

/// Whether a reconnect discarded work this client had not sent yet.
///
/// Sticky on purpose. Every other collaboration status describes a condition
/// that will pass; this one describes cells that are gone, and the "live" the
/// reconnect would otherwise emit a moment later erases the only notice the
/// user ever gets. The transport withholds "live" until an edit made *after*
/// the loss has been acknowledged, so this clears when editing demonstrably
/// works again rather than when the socket merely reopens.
let collabLostUnsent = false;

/// Join a collaborative session.
///
/// `url` is the server's WebSocket endpoint and `token` the host-signed token
/// that says who this is and what they may do. Returns the session handle, or
/// throws if one is already open — joining twice would leave two transports
/// submitting the same edits under different client ids.
export async function collaborate({ url, token, document: documentKey, onStatus, onDocument, onPresence } = {}) {
  if (collabSession) throw new Error("already in a collaborative session");
  const { collaborate: connect } = await import(`./collab.js?b=${BUILD}`);
  collabSession = connect({
    url,
    token,
    document: documentKey,
    wasm,
    onStatus: (event) => {
      // The status line is the only place the editor says this out loud, and
      // "reconnecting" is the one a user needs to see before they wonder why
      // their typing stopped mattering.
      //
      // `lost` outranks the rest and **sticks**: it is the one state that
      // reports work already gone rather than a condition that will pass, so
      // letting the next "collaborating" overwrite it turns a data-loss notice
      // into a flicker. It clears when the user next edits successfully, which
      // is the point at which they have demonstrably read the grid.
      if (event.state === "lost") {
        collabLostUnsent = true;
        status.textContent =
          "reconnected to a different server — edits you made while disconnected were not saved";
      } else if (event.state === "live") {
        collabLostUnsent = false;
        status.textContent = "collaborating";
      } else if (collabLostUnsent) {
        // Held. Anything else this transport wants to say can wait: the
        // transport does not send "live" again until an edit made *after* the
        // loss has been acknowledged, so this clears when editing demonstrably
        // works, not merely when the socket comes back.
      } else if (event.state === "reconnecting") status.textContent = "reconnecting…";
      else if (event.state === "refused") status.textContent = `not saved: ${event.detail}`;
      else if (event.state === "stopped") status.textContent = `disconnected: ${event.detail}`;

      // **A session the transport has finished with is finished here too.**
      //
      // `stopped` is terminal — the transport has closed the socket and will
      // not reconnect, because reconnecting would be refused for the same
      // reason. But only an explicit `stopCollaborating()` used to clear
      // `collabSession`, so the editor went on believing it was in a session
      // that no longer existed, and the next `collaborate()` threw "already in
      // a collaborative session".
      //
      // The result: a token that expired, or any other refusal, could only be
      // recovered from by reloading the page — which throws away whatever the
      // user had locally. Observed live: a refused join left the editor unable
      // to rejoin the same document with a valid token seconds later.
      if (event.state === "stopped") {
        collabSession = null;
        collabRoster.clear();
        forgetCollabAnnouncement();
        renderPresence();
        draw();
      }
      onStatus?.(event);
    },
    onDocument: (event) => {
      adoptCollabDocument(event);
      onDocument?.(event);
    },
    onPresence: (event) => {
      if (event.kind === "gone") collabRoster.delete(event.client);
      else collabRoster.set(event.client, event);
      renderPresence();
      draw();
      onPresence?.(event);
    },
  });
  // Shown as soon as there is a session, before anybody else has moved: "only
  // you" is an answer to "who is collaborating", and an absent control is not.
  renderPresence();
  return collabSession;
}

/// Leave, if in a session. Safe to call when not.
export function stopCollaborating() {
  collabSession?.close();
  collabSession = null;
  collabRoster.clear();
  forgetCollabAnnouncement();
  renderPresence();
}

/// Forget what was last announced, and cancel anything queued to announce.
///
/// Both halves matter when a session ends. The key has to go or the *next*
/// session's first selection looks unchanged and is never sent; the timer has
/// to go or it fires into a closed session, and — worse — a draft queued from
/// the old session would be the first thing the new one said.
function forgetCollabAnnouncement() {
  collabAnnounced = "";
  collabAnnouncedAt = 0;
  clearTimeout(collabAnnounceTimer);
  collabAnnounceTimer = null;
}

/// The other participants, as a host would show them.
export function collaborators() {
  return [...collabRoster.values()];
}

// --- The participant roster (COL-33) ----------------------------------------
//
// `drawCollaborators` puts a cursor where somebody is standing, which answers
// "who is in this cell" and nothing else. It cannot answer "who is in this
// document" — a cursor two thousand rows down or on another sheet is drawn
// nowhere — and that is what was reported against the running demo: *"i can't
// see here which profiles are collaborating... i see the name"*. The name was
// the only evidence anybody else existed, and only if you happened to be
// looking at their cell.
//
// So: a face stack in the menu bar that opens a roster of who is here, what
// they are doing, and a click that takes you to them.
//
// **Every string in here is somebody else's text.** A name arrives in the
// token, which an integrator minted from whatever their user typed; a sheet
// name comes out of a workbook, which is a file somebody uploaded. SEC-001 is
// the rule that neither may build DOM in this origin, so this constructs nodes
// and assigns `textContent`. There is no markup in this section, and there is
// no interpolation into a selector either — a client id with a quote in it
// would otherwise throw out of `querySelector` and take the roster with it.
//
// Nothing below touches the document, the history or the outgoing log.

/// How many faces the stack shows before the rest become "+n".
///
/// Three, because the stack shares a 30px bar with eight menus and the collapse
/// caret, and every extra face is 13px taken from them. Everybody is in the
/// list; the stack is a summary of it, not the roster.
const PRESENCE_FACES = 3;

/// Whether the roster popup is open.
let presenceOpen = false;

/// A participant's display name, never empty.
///
/// `someone` matches what the cursor tag draws for a nameless participant, so
/// the list and the grid agree about who that is.
function participantName(who) {
  const name = typeof who?.name === "string" ? who.name.trim() : "";
  return name || t("presence.someone", "someone");
}

/// One or two letters for a face, from a name that may be anything at all.
///
/// `Array.from` rather than `name[0]`: a name starting with an emoji or any
/// astral-plane character has a first *code unit* that is half a surrogate
/// pair, and half a pair renders as a replacement box.
function participantInitials(name) {
  const words = String(name ?? "").trim().split(/\s+/).filter(Boolean);
  const first = (w) => Array.from(w)[0] ?? "";
  const out = words.length > 1 ? first(words[0]) + first(words[1]) : first(words[0] ?? "");
  return (out || "?").toUpperCase();
}

/// The r/g/b of a colour `participantColor` has already vouched for, or null.
function participantChannels(color) {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color);
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((c) => c + c).join("") : hex[1];
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
  }
  // A named colour or an `rgb()`, which `participantColor` passes through when
  // the browser agrees it is a colour. Ask the browser what it resolved to.
  const probe = new Option().style;
  probe.color = color;
  const m = /^rgba?\(([^)]+)\)$/.exec(probe.color);
  if (!m) return null;
  const parts = m[1].split(/[,\s/]+/).filter(Boolean).map(Number).slice(0, 3);
  return parts.length === 3 && parts.every(Number.isFinite) ? parts : null;
}

/// Black or white on a participant's colour, whichever can be read.
///
/// Computed rather than assumed, because the palette is the server's and a
/// deployment may replace it: white initials on `#FDD835` are invisible, and an
/// unreadable name is the exact failure this control exists to fix.
function participantInk(color) {
  const rgb = participantChannels(color);
  if (!rgb) return "#ffffff";
  // Rec. 709 luma — green carries most of the perceived brightness, which a
  // plain average of the channels gets wrong for yellows and blues.
  const luma = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255;
  return luma > 0.6 ? "#0b0d12" : "#ffffff";
}

/// A coloured initial for the stack and for each row of the list.
function participantFace(who) {
  const color = participantColor(who.color);
  const face = el("span", "presence-face", participantInitials(who.name));
  // Assigned through CSSOM, which parses a *value* and drops what it cannot
  // parse. It cannot become markup, and `participantColor` has already refused
  // anything the browser does not read as a colour.
  face.style.background = color;
  face.style.color = participantInk(color);
  if (who.editing) face.classList.add("typing");
  return face;
}

/// A cell reference from a presence entry, or null.
///
/// Every field is validated rather than trusted. Presence comes from another
/// client by way of the server, which bounds it but does not promise it is
/// sensible, and `A1(undefined, undefined)` is a label reading "undefined".
function presenceCell(who) {
  const ok = (n) => Number.isInteger(n) && n >= 0;
  const draft = who?.editing;
  // A draft wins over the selection: while a formula is being written the
  // selection walks off to pick references (see `collabDraft`), so the cell
  // they are *in* is the one they are typing into, not the one highlighted.
  if (draft && Array.isArray(draft.at) && draft.at.length === 2 && draft.at.every(ok)) {
    return { r0: draft.at[0], c0: draft.at[1], r1: draft.at[0], c1: draft.at[1] };
  }
  const sel = who?.selection;
  if (!Array.isArray(sel) || sel.length !== 4 || !sel.every(ok)) return null;
  return {
    r0: Math.min(sel[0], sel[2]),
    c0: Math.min(sel[1], sel[3]),
    r1: Math.max(sel[0], sel[2]),
    c1: Math.max(sel[1], sel[3]),
  };
}

/// Where a participant is, as a person would say it: `Budget!D8`.
function presenceWhere(who) {
  const sheet = Number.isInteger(who?.sheet) ? who.sheet : -1;
  const named = sheet >= 0 ? sheetNameAt(sheet) : null;
  // A sheet index this client cannot name is still worth reporting: it means
  // they are on a sheet that was added or removed under us, and "Sheet 3" is
  // more use than an empty line.
  const where = typeof named === "string" && named ? named : `${t("presence.sheet", "Sheet")} ${sheet + 1}`;
  const at = presenceCell(who);
  const a = at ? A1(at.r0, at.c0) : "";
  const b = at ? A1(at.r1, at.c1) : "";
  const ref = !at ? "" : a === b ? a : `${a}:${b}`;
  return {
    text: ref ? `${where}!${ref}` : where,
    // Their sheet is not the one on screen, so going to them means leaving it.
    elsewhere: sheet >= 0 && sheet !== state.sheet,
  };
}

/// One row of the roster: who, where, and whether they are mid-word.
function presenceRow(who) {
  const name = participantName(who);
  const where = presenceWhere(who);
  const row = el("button", "presence-item");
  row.type = "button";
  row.setAttribute("role", "menuitem");
  // Read back after a rebuild to restore focus; never interpolated into a
  // selector, because a client id is not this editor's string to trust.
  row.dataset.client = String(who.client ?? "");
  if (where.elsewhere) row.classList.add("elsewhere");
  row.appendChild(participantFace(who));
  const who2 = el("span", "presence-who");
  who2.appendChild(el("span", "presence-name", name));
  who2.appendChild(el("span", "presence-where", where.text));
  row.appendChild(who2);
  const typing = who.editing ? t("presence.typing", "typing") : "";
  if (typing) row.appendChild(el("span", "presence-typing", typing));
  // The visible text plus what the row does, because "Grace Hopper Budget!D8"
  // read aloud does not say that activating it goes there.
  row.setAttribute(
    "aria-label",
    `${t("presence.goto", "Go to")} ${name}, ${where.text}${typing ? `, ${typing}` : ""}`,
  );
  row.addEventListener("click", () => {
    closePresence();
    jumpToParticipant(who);
  });
  return row;
}

/// Rebuild the face stack and the roster from the live roster map.
///
/// Called on every presence message, which during a burst of typing is roughly
/// six a second. That is why the open list restores focus and scroll position
/// afterwards: somebody arrow-keying down the roster while a peer types would
/// otherwise have the focus thrown back to the page under them, and a list that
/// jumps to the top every time somebody presses a key cannot be read.
function renderPresence() {
  const box = byId("presence");
  const btn = byId("presence-btn");
  const faces = byId("presence-faces");
  const label = byId("presence-label");
  const menu = byId("presence-menu");
  // A host that removed this chrome, or a call before the mount is bound.
  if (!box || !btn || !faces || !label || !menu) return;

  // No session, nothing to say. The editor is single-player most of the time
  // and a permanent "only you" chip is noise in a bar that carries none.
  if (!collabSession) {
    box.hidden = true;
    if (presenceOpen) closePresence();
    return;
  }
  box.hidden = false;

  // Sorted by name, not by arrival: the list is read to *find* somebody, and
  // an order that changes as people move is an order nobody can search.
  const others = collaborators().slice().sort((a, b) => {
    const byName = participantName(a).localeCompare(participantName(b));
    return byName || String(a.client ?? "").localeCompare(String(b.client ?? ""));
  });

  faces.textContent = "";
  for (const who of others.slice(0, PRESENCE_FACES)) faces.appendChild(participantFace(who));
  if (others.length > PRESENCE_FACES) {
    faces.appendChild(
      el("span", "presence-face presence-more", `+${others.length - PRESENCE_FACES}`),
    );
  }

  const count = others.length;
  label.textContent =
    count === 0
      ? t("presence.alone", "Only you")
      : count === 1
        ? t("presence.one", "1 other")
        : t("presence.many", "%n others").replace("%n", String(count));
  // `setTip` writes the tooltip *and* the accessible name, through whichever
  // surface this control ended up using. The names are in it because a screen
  // reader user should not have to open a menu to learn whether they are alone.
  setTip(
    btn,
    count === 0
      ? t("presence.tip-alone", "Collaborators — you are the only one here")
      : `${t("presence.tip", "Collaborators")} — ${label.textContent}: ${others
          .map(participantName)
          .join(", ")}`,
  );

  // The list itself is only built while it is on screen. Presence arrives about
  // six times a second per person who is typing, and rebuilding twenty rows
  // nobody is looking at, sixscore times a second, is work the grid wants for
  // drawing. `openPresence` builds it on the way open.
  if (!presenceOpen) {
    menu.textContent = "";
    return;
  }

  // What had focus, so a rebuild under an open menu does not steal it.
  const focused = presenceItems().includes(activeEl()) ? activeEl().dataset.client : null;
  const scrolled = menu.scrollTop;
  menu.textContent = "";
  if (!others.length) {
    // A disabled item rather than a bare line of text: a `role="menu"` with no
    // `menuitem` in it is a menu a screen reader reads as empty, which is not
    // the same thing as being told you are on your own.
    const empty = el("div", "presence-empty", t("presence.empty", "You are the only one here."));
    empty.setAttribute("role", "menuitem");
    empty.setAttribute("aria-disabled", "true");
    menu.appendChild(empty);
  } else {
    for (const who of others) menu.appendChild(presenceRow(who));
  }
  menu.scrollTop = scrolled;
  if (focused !== null) {
    for (const item of presenceItems()) {
      if (item.dataset.client === focused) { item.focus(); break; }
    }
  }
}

/// The focusable rows of the open roster, in the order they are shown.
function presenceItems() {
  const menu = byId("presence-menu");
  return menu ? [...menu.querySelectorAll(".presence-item")] : [];
}

function openPresence() {
  const menu = byId("presence-menu");
  const btn = byId("presence-btn");
  if (!menu || !btn || !collabSession) return;
  presenceOpen = true;
  renderPresence(); // current as of the moment it opens, not of the last event
  menu.hidden = false;
  btn.setAttribute("aria-expanded", "true");
}

/// Close the roster. `refocus` returns focus to the button, which is what
/// Escape must do — closing a menu that had focus and dropping focus on the
/// floor strands a keyboard user at the top of the document.
function closePresence(refocus = false) {
  const menu = byId("presence-menu");
  const btn = byId("presence-btn");
  presenceOpen = false;
  if (menu) menu.hidden = true;
  if (btn) {
    btn.setAttribute("aria-expanded", "false");
    if (refocus) btn.focus();
  }
}

/// Take this user to what a participant has selected.
///
/// Being told somebody is in P47 and having to go and find P47 is half a
/// feature, and it is the half the roster would otherwise be.
///
/// It moves the **view**, never this user's selection. Your active cell is
/// where your next keystroke lands, and a control that quietly moved it would
/// be a control that quietly types your work somewhere else. Their cursor is
/// already painted by `drawCollaborators`, so arriving is enough to see them.
///
/// Nothing here writes to the document, the history or the outgoing log.
/// Switching sheets does announce *this* client's own presence — the same
/// message `switchSheet` already sends when a tab is clicked, from the same
/// code, because this client really did move.
function jumpToParticipant(who) {
  const sheet = Number.isInteger(who?.sheet) ? who.sheet : -1;
  if (sheet >= 0 && sheet !== state.sheet) {
    let count = 0;
    try { count = JSON.parse(wasm.session_sheet_names()).length; } catch {}
    // A sheet this client does not have is one that was deleted under it (or
    // added and not yet applied): there is nowhere to go, so the jump keeps the
    // view it has rather than switching to an index that does not exist.
    if (sheet < count) switchSheet(sheet);
  }
  const at = presenceCell(who);
  if (at) {
    // `ensureVisible` twice rather than fresh geometry: it is the same scroll
    // every other jump in this editor performs, so it cannot disagree with
    // them, and it moves the minimum — a participant already on screen does not
    // throw the view around. Far corner first so the near one wins when their
    // range is bigger than the viewport; the top-left of a block is the part
    // you want to be looking at.
    ensureVisible(at.r1, at.c1);
    ensureVisible(at.r0, at.c0);
  }
  draw();
  canvas?.focus();
  // Said out loud, because for a screen-reader user the whole effect of this
  // click is a canvas that scrolled.
  if (liveEl) liveEl.textContent = `${participantName(who)} — ${presenceWhere(who).text}`;
}

/// Wire the roster control. Called once, from `wireEvents`.
function wirePresence() {
  const box = byId("presence");
  const btn = byId("presence-btn");
  const menu = byId("presence-menu");
  if (!box || !btn || !menu) return;

  // Put back where it belongs: `buildMenuBar()` appends File…Help *after*
  // whatever the markup held, and `hdr-collapse` re-appends itself last, so
  // relying on markup order would leave the roster to the left of the File
  // menu. Right-most but one, beside the collapse caret.
  const bar = byId("menubar");
  if (bar) bar.insertBefore(box, byId("hdr-collapse") ?? null);

  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (presenceOpen) closePresence(); else openPresence();
  });
  btn.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      openPresence();
      const items = presenceItems();
      (e.key === "ArrowDown" ? items[0] : items[items.length - 1])?.focus();
      e.preventDefault();
    } else if (e.key === "Escape" && presenceOpen) {
      closePresence();
      e.preventDefault();
    }
  });

  // Outside-click closes, like every other popover here. `composedPath` rather
  // than `contains(e.target)`: inside a shadow root a click is retargeted to
  // the host element, so `contains` answers "no" for clicks that were in fact
  // ours — the path is the one view of the click that crosses the boundary.
  document.addEventListener("click", (e) => {
    if (!presenceOpen) return;
    const path = e.composedPath ? e.composedPath() : [e.target];
    if (!path.includes(box)) closePresence();
  });

  menu.addEventListener("keydown", (e) => {
    const items = presenceItems();
    const at = items.indexOf(activeEl());
    const step = (i) => { items[((i % items.length) + items.length) % items.length]?.focus(); };
    if (e.key === "ArrowDown") { step(at + 1); e.preventDefault(); }
    else if (e.key === "ArrowUp") { step(at - 1); e.preventDefault(); }
    else if (e.key === "Home") { step(0); e.preventDefault(); }
    else if (e.key === "End") { step(items.length - 1); e.preventDefault(); }
    else if (e.key === "Escape") { closePresence(true); e.preventDefault(); }
    // Tab out of a menu closes it. Not prevented: the focus is going somewhere
    // sensible on its own, and trapping it here would trap it for good.
    else if (e.key === "Tab") closePresence();
  });

  renderPresence();
}

/// Take on what the transport just did to the model.
function adoptCollabDocument(event) {
  if (event.reason === "joined") {
    // The whole workbook was replaced by the session's snapshot, so this is the
    // same refresh a file open needs and for the same reason — every cache
    // below is keyed to a document that is no longer there.
    invalidateGrowth();
    imageCache.clear();
    syncClock();
    try { state.sheet = wasm.session_active_sheet(); } catch { state.sheet = 0; }
    state.scrollX = state.scrollY = 0;
    renderTabs();
    select(0, 0);
    // The engine refuses the edit, not the toolbar. A viewer whose buttons were
    // merely hidden is one bug away from editing a document they may not.
    if (event.editable === false) setReadOnly(true);
    return;
  }
  // A remote edit. Cheaper than a join — the model is continuous — but the
  // sheet list can have changed too, since adding or renaming one is an
  // ordinary operation like any other.
  invalidateGrowth();
  renderTabs();
  draw();
}

/// Paint the other participants — their selection, what they are typing, and a
/// name tag — each in their own colour.
///
/// The roster was already being kept — presence arrives, `collabRoster` is
/// updated and `draw()` is called — and then nothing read it. Co-editing worked
/// and looked exactly like editing alone, which is the failure this fixes: the
/// point of seeing somebody else's cursor is knowing not to type there.
///
/// Colour and name both come from the server, which takes them from the token.
/// A client naming itself is the one place a claimed identity would be believed.
/// A participant's colour as canvas will actually accept it.
///
/// The server's palette is bare hex — `0891B2`, no `#` — and an invalid
/// `strokeStyle` is **silently ignored** by canvas rather than throwing, so
/// every cursor would have quietly inherited whatever colour was set last. Every
/// participant in the accent colour looks like a working feature, which is the
/// kind of wrong that never gets reported.
function participantColor(raw) {
  if (typeof raw !== "string" || !raw) return colors.accent;
  const hex = raw.trim();
  if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex)) return hex;
  if (/^([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex)) return `#${hex}`;
  // Anything else — a named colour, an rgb() — is passed through only if the
  // browser agrees it is a colour, so a malformed token cannot blank a cursor.
  const probe = new Option().style;
  probe.color = hex;
  return probe.color ? hex : colors.accent;
}

function drawCollaborators(v, perQuad) {
  if (!collabRoster.size) return;
  for (const who of collabRoster.values()) {
    // Only this sheet. A cursor on another tab is real and is not here.
    if (who.sheet !== state.sheet) continue;
    const color = participantColor(who.color);
    // Before the selection, and not inside its visibility test: while a formula
    // is being written the selection walks off to pick references, so the cell
    // being typed into can be on screen when the cursor is not. The draft is the
    // part somebody needs to see.
    drawCollaboratorDraft(who, color, v, perQuad);

    const sel = who.selection;
    if (!Array.isArray(sel) || sel.length !== 4) continue;
    const [r0, c0, r1, c1] = sel;
    const sx = spanX(Math.min(c0, c1), Math.max(c0, c1), v);
    const sy = spanY(Math.min(r0, r1), Math.max(r0, r1), v);
    // Scrolled out of view, or collapsed to nothing by the pane clamp.
    if (sx.w <= 0 || sy.h <= 0) continue;

    perQuad(() => {
      ctx.save();
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.strokeRect(sx.x + 1, sy.y + 1, Math.max(1, sx.w - 1), Math.max(1, sy.h - 1));
      // A wash, so a range reads as theirs without hiding the values in it.
      ctx.globalAlpha = 0.12;
      ctx.fillStyle = color;
      ctx.fillRect(sx.x + 1, sy.y + 1, Math.max(1, sx.w - 1), Math.max(1, sy.h - 1));
      ctx.restore();
    });

    const name = typeof who.name === "string" && who.name ? who.name : "someone";
    perQuad(() => {
      ctx.save();
      // Both of these leak from cell drawing, which sets them per cell. The
      // label came out centred on its own left edge — "Guest 21" rendered as
      // "st 21", the rest of it painted white-on-white outside the tag — and
      // nothing about the code said so. Only the screenshot did.
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      ctx.font = "11px system-ui, sans-serif";
      const tw = Math.ceil(ctx.measureText(name).width);
      const tagW = tw + 8;
      const tagH = 15;
      // Above the range, and below it when that would leave the grid — a label
      // clipped off the top of the canvas names nobody.
      const above = sy.y - tagH >= HH;
      const ty = above ? sy.y - tagH : sy.y + sy.h;
      // Clamped so a range running off the right edge keeps its label on screen.
      const tx = Math.max(HW, Math.min(sx.x, v.w - tagW));
      ctx.fillStyle = color;
      ctx.fillRect(tx, ty, tagW, tagH);
      ctx.fillStyle = "#fff";
      ctx.fillText(name, tx + 4, ty + tagH / 2 + 0.5);
      ctx.restore();
    });
  }
}

/// Paint what a participant is typing, in the cell they are typing it into.
///
/// The point of COL-35, in one function: until this existed, somebody else's
/// work appeared only when they pressed Enter, so two people could fill the same
/// cell and neither found out until one of them lost.
///
/// Drawn as their in-cell editor would look on their screen — an opaque box in
/// their colour over whatever the cell currently holds — because that is what it
/// is. A wash over the old value would leave two overlapping strings and read as
/// a rendering fault.
///
/// **The text is untrusted.** It came from another participant, through the
/// server, which bounds its length and does not otherwise vouch for it. It goes
/// to `fillText` on the canvas and never near markup: SEC-001 is the rule, and
/// this is the newest path in the editor that carries somebody else's text.
function drawCollaboratorDraft(who, color, v, perQuad) {
  const draft = who.editing;
  if (!draft || !Array.isArray(draft.at) || draft.at.length !== 2) return;
  const [row, col] = draft.at;
  const dx = spanX(col, col, v);
  const dy = spanY(row, row, v);
  // Scrolled out of view, or hidden, or clamped away by a frozen pane.
  if (dx.w <= 0 || dy.h <= 0) return;
  const text = typeof draft.text === "string" ? draft.text : "";

  perQuad(() => {
    ctx.save();
    // Opaque, so the committed value underneath does not show through and
    // double up with the draft over it.
    ctx.fillStyle = colors.bg;
    ctx.fillRect(dx.x, dy.y, dx.w, dy.h);
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.strokeRect(dx.x + 1, dy.y + 1, Math.max(1, dx.w - 2), Math.max(1, dy.h - 2));

    // Clipped to the cell. A long formula must not spill across the neighbours
    // the way a committed overflowing value may — those cells belong to other
    // people's values, and this text is not committed to anything.
    ctx.beginPath();
    ctx.rect(dx.x + 2, dy.y + 2, Math.max(0, dx.w - 4), Math.max(0, dy.h - 4));
    ctx.clip();
    ctx.fillStyle = colors.fg;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.font = "12px system-ui, sans-serif";
    // The first line only. A draft with a hard line break in it is legitimate
    // (Alt+Enter) and a preview an inch tall is not.
    const line = text.split("\n")[0];
    ctx.fillText(line, dx.x + 4, dy.y + dy.h / 2);
    // A caret at the end of it, so an empty draft still reads as "somebody is
    // in here typing" rather than as a blank cell with a border.
    const caret = Math.min(dx.x + 4 + Math.ceil(ctx.measureText(line).width) + 1, dx.x + dx.w - 3);
    ctx.fillStyle = color;
    ctx.fillRect(caret, dy.y + 3, 1.5, Math.max(2, dy.h - 6));
    ctx.restore();
  });
}

/// What this participant is part-way through typing, for the others to watch.
///
/// Null unless a cell editor is open on *this* sheet. The draft belongs to
/// `editHome` rather than to the selection, because writing a formula walks the
/// selection off to pick references — and a peer would otherwise watch the text
/// hop around the grid with it.
///
/// The cross-sheet case reports nothing rather than guessing: a presence message
/// names one sheet, and a draft on the sheet you are not looking at is not
/// something a grid can draw. It comes back the moment the author does.
function collabDraft() {
  if (!editSurface || !editHome) return null;
  if (editHome.sheet !== state.sheet) return null;
  return { at: [editHome.row, editHome.col], text: editSurface.value };
}

/// Tell the others where this participant is looking and what they are typing,
/// when either has changed.
///
/// Called from `draw()` — which covers every way a selection can move — and
/// directly from the editing path, which `draw()` does not cover: a keystroke
/// changes the text without changing anything the grid repaints.
///
/// Presence is ephemeral and never transformed (ADR-011), so sending it late
/// costs nothing and losing one costs nothing either. That is what lets this be
/// throttled at all.
function announceCollabSelection() {
  if (!collabSession) return;
  const r = selRect();
  const draft = collabDraft();
  const key = `${state.sheet}:${r.r0},${r.c0},${r.r1},${r.c1}|${
    draft ? `${draft.at[0]},${draft.at[1]}:${draft.text}` : ""
  }`;
  if (key === collabAnnounced) return;

  const since = Date.now() - collabAnnouncedAt;
  if (since < PRESENCE_THROTTLE_MS) {
    // Too soon. Come back at the end of the window and read the state *then*,
    // rather than sending this one late — by then the user will have typed
    // more, and what everybody wants to see is where they got to.
    if (!collabAnnounceTimer) {
      collabAnnounceTimer = setTimeout(() => {
        collabAnnounceTimer = null;
        announceCollabSelection();
      }, PRESENCE_THROTTLE_MS - since);
    }
    return;
  }
  collabAnnounced = key;
  collabAnnouncedAt = Date.now();
  collabSession.present(state.sheet, [r.r0, r.c0, r.r1, r.c1], draft);
}

/// Distinguishes this module instance's engine from any other on the page.
///
/// The editor is a module with module-scope state — one `wasm` binding, one
/// `state`, one geometry cache — so a second element mounting the *same* module
/// would share and race all of it. Each element therefore imports its own copy
/// of this module and its own copy of the wasm glue, which is what this key
/// varies. See `docs/55` §4b for what that costs.
let instanceKey = "";

/// Fetch and register the faces a host offers, when it says it offers some.
///
/// **Opt-in, and it has to be.** The obvious version probes `/api/fonts` on
/// every boot and treats a 404 as "no font service" — which works, and logs
/// `Failed to load resource: 404` to the console of every deployment that does
/// not run one, which is most of them. A `fetch` rejection can be caught; the
/// browser logging a failed request cannot. A diagnostic that cries wolf on
/// every boot is worse than no diagnostic, so nothing is fetched unless asked.
///
/// Ask with `?fonts` on the editor URL — bare for the conventional
/// `/api/fonts`, or `?fonts=/some/other/path` for anything else.
///
/// Failures past that point are logged and never fatal: having been told the
/// service is there, a face that will not load is worth a line, because the
/// realistic cause is a fetch that returned an error page and the symptom
/// otherwise shows up much later looking like a renderer bug.
async function registerSuppliedFonts() {
  const asked = new URL(location.href).searchParams.get("fonts");
  if (asked === null) return;
  const service = asked === "" ? "/api/fonts" : asked;
  try {
    const response = await fetch(service, { headers: { accept: "application/json" } });
    if (!response.ok) {
      console.warn(`[opencalc] font service ${service} answered ${response.status}`);
      return;
    }
    // URLs, taken as given and resolved against this page. The service decides
    // where its faces live; this only fetches them.
    const { fonts = [] } = await response.json();
    for (const url of fonts) {
      try {
        const face = await fetch(new URL(url, location.href));
        if (!face.ok) {
          console.warn(`[opencalc] ${url} answered ${face.status}; not registered`);
          continue;
        }
        const bytes = new Uint8Array(await face.arrayBuffer());
        if (!wasm.register_font(bytes)) {
          console.warn(`[opencalc] ${url} is not a readable font face; ignored`);
        }
      } catch (why) {
        console.warn(`[opencalc] could not register ${url}:`, why);
      }
    }
  } catch (why) {
    console.warn(`[opencalc] font service ${service} unreachable:`, why);
  }
}

async function main() {
  bindElements();
  const mod = await import(`./pkg/casual_calc_wasm.js?b=${BUILD}${instanceKey}`);
  init = mod.default;
  wasm = mod;
  // Resolved against **this module**, not the page. A bare relative string is
  // resolved against the document, so the binary was fetched from wherever the
  // host's HTML happened to live — which worked only while the page sat beside
  // the engine, and 404'd the moment an example in another directory embedded
  // it. `import.meta.url` is the whole reason a bundler can emit this as an
  // asset and still have it found.
  await init(new URL(`./pkg/casual_calc_wasm_bg.wasm?b=${BUILD}`, import.meta.url));

  // A handle for a host that embeds this page rather than importing the module
  // itself — an iframe's exports cannot be reached from the parent frame, and a
  // host has no way to name the cache-busted specifier this module was loaded
  // under. Importing `import.meta.url` returns this same instance from the
  // module cache rather than building a second one.
  if (typeof window !== "undefined") {
    try { window.opencalcEditor = await import(import.meta.url); } catch {}
  }

  // Faces the deployment supplies, registered before anything can be rendered
  // to a PNG. Best-effort on purpose: a host that offers no `/api/fonts` is the
  // normal case, and the editor itself never needs them — the browser draws
  // every cell a user looks at with its own faces. This is for `render_png`,
  // where a missing face is a picture full of boxes.
  await registerSuppliedFonts();

  COL_W = wasm.default_col_px();
  ROW_H = wasm.default_row_px();
  readColors();
  wasm.session_new();
  // Before anything is evaluated: nothing volatile works until the host has
  // handed the engine a clock.
  syncClock(true);
  wireEvents();
  // Toolbar controls get command ids from their element ids (`tb-bold` →
  // `toolbar.bold`), so a host hides a button by name rather than by reaching
  // into the shadow root with a selector that will move.
  for (const node of qsa("[id^='tb-']")) {
    if (!node.dataset.ocCommand) node.dataset.ocCommand = `toolbar.${node.id.slice(3)}`;
  }
  // Tooltips are the toolbar's only text, so they are what a translated
  // toolbar translates. The English one is kept as the fallback.
  //
  // Read from `data-tip` as well as `title`: the custom tooltip layer runs
  // first and *moves* `title` onto `data-tip` to suppress the native bubble,
  // so by the time this ran only the handful of nodes outside its reach still
  // had a `title` at all. Sixty-four of the seventy-five tooltips were
  // therefore never registered for translation, and `tip.*` — a documented
  // part of the SDK's localization surface — silently did nothing for them.
  for (const node of qsa("[title], [data-tip]")) {
    if (!node.dataset.ocTip) node.dataset.ocTip = node.dataset.tip || node.title;
  }
  relabel();
  seed();
  renderTabs();
  resize();
  status.textContent = `engine v${wasm.version()}`;
}

/// Start the editor against the current mount root.
///
/// Exported so an embed wrapper can point the editor at a shadow root first;
/// the page host below starts it immediately, as it always did.
export async function start(key = "") {
  instanceKey = key ? `&i=${encodeURIComponent(key)}` : "";
  return main();
}

// A page mount starts itself. An embedded one imports this module, calls
// `setMountRoot`, and then `start` — by which time this has already run against
// a document with no `#grid` in it, so it is skipped.
if (byId("grid")) {
  main().catch((err) => {
    if (status) status.textContent = `failed: ${err}`;
    else console.error(err);
  });
}

/// Where the grid's clickable chrome is, in canvas pixels, for the browser gate.
///
/// Returns the geometry a *user* aims at — the hidden-band handles and the two
/// corner freeze handles — so a test clicks the same pixels rather than
/// re-deriving them from assumptions about header widths. The alternative is a
/// test that hunts for a five-pixel target and goes flaky the first time a
/// default changes.
// The personal-view state a browser test needs (COL-32). Row visibility is
// asked of the *engine*, not of the DOM, because a row hidden by a personal
// view collapses to zero pixels and "did not render" is indistinguishable from
// "rendered somewhere else".
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

// Apply a personal filter without driving the dropdown, so a test can assert
// the *policy* (nothing relayed, nothing saved, subtotal unmoved) rather than
// the menu's markup.
export function personalFilterForTest(col, values) {
  applyPersonalFilter(col, values);
  afterFilterChange();
}

// Clearing, and opening a column's dropdown, from a test. Both go through the
// same functions the menu and the header button call, so a test cannot pass
// against a path a user never takes.
export function clearMyViewForTest() {
  clearMyView();
}

export function openColumnFilterForTest(col) {
  openColumnFilter(col, 100, 100);
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
