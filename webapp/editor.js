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
  firstRow: 0,
  firstCol: 0,
  sel: { row: 0, col: 0 }, // focus cell
  anchor: { row: 0, col: 0 }, // selection anchor
  dragging: false,
  editing: false,
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

function visible() {
  const rect = wrap.getBoundingClientRect();
  return {
    w: rect.width,
    h: rect.height,
    cols: Math.ceil((rect.width - HW) / COL_W) + 1,
    rows: Math.ceil((rect.height - HH) / ROW_H) + 1,
  };
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
  const v = visible();
  ctx.clearRect(0, 0, v.w, v.h);
  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, v.w, v.h);

  // Selection highlight (behind text): the whole range, then the focus cell.
  const rectSel = selRect();
  const selX = HW + (rectSel.c0 - state.firstCol) * COL_W;
  const selY = HH + (rectSel.r0 - state.firstRow) * ROW_H;
  const selW = (rectSel.c1 - rectSel.c0 + 1) * COL_W;
  const selH = (rectSel.r1 - rectSel.r0 + 1) * ROW_H;
  ctx.fillStyle = colors.sel;
  ctx.fillRect(Math.max(HW, selX), Math.max(HH, selY), selW, selH);
  const sx = HW + (state.sel.col - state.firstCol) * COL_W;
  const sy = HH + (state.sel.row - state.firstRow) * ROW_H;

  // Gridlines.
  ctx.strokeStyle = colors.grid;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let c = 0; c <= v.cols; c++) {
    const x = Math.floor(HW + c * COL_W) + 0.5;
    ctx.moveTo(x, HH);
    ctx.lineTo(x, v.h);
  }
  for (let r = 0; r <= v.rows; r++) {
    const y = Math.floor(HH + r * ROW_H) + 0.5;
    ctx.moveTo(HW, y);
    ctx.lineTo(v.w, y);
  }
  ctx.stroke();

  // Headers.
  ctx.fillStyle = colors.headerBg;
  ctx.fillRect(0, 0, v.w, HH);
  ctx.fillRect(0, 0, HW, v.h);
  ctx.fillStyle = colors.muted;
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  for (let c = 0; c < v.cols; c++) {
    const col = state.firstCol + c;
    ctx.fillText(colName(col), HW + c * COL_W + COL_W / 2, HH / 2);
  }
  for (let r = 0; r < v.rows; r++) {
    const row = state.firstRow + r;
    ctx.fillText(String(row + 1), HW / 2, HH + r * ROW_H + ROW_H / 2);
  }

  // Cell text (from the engine).
  const lastRow = state.firstRow + v.rows;
  const lastCol = state.firstCol + v.cols;
  const items = JSON.parse(
    wasm.session_cells(state.sheet, state.firstRow, state.firstCol, lastRow, lastCol),
  );
  ctx.textBaseline = "middle";
  // Fills first (behind text), so a colored empty cell still shows.
  for (const it of items) {
    if (!it.bg) continue;
    const x = HW + (it.c - state.firstCol) * COL_W;
    const y = HH + (it.r - state.firstRow) * ROW_H;
    ctx.fillStyle = "#" + it.bg;
    ctx.fillRect(x + 1, y + 1, COL_W - 1, ROW_H - 1);
  }
  for (const it of items) {
    if (!it.t) continue;
    const x = HW + (it.c - state.firstCol) * COL_W;
    const y = HH + (it.r - state.firstRow) * ROW_H + ROW_H / 2;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y - ROW_H / 2, COL_W, ROW_H);
    ctx.clip();
    const weight = it.b ? "600 " : "";
    const slant = it.i ? "italic " : "";
    ctx.font = `${slant}${weight}13px system-ui, sans-serif`;
    ctx.fillStyle = it.fc ? "#" + it.fc : colors.fg;
    if (it.a === "r") {
      ctx.textAlign = "right";
      ctx.fillText(it.t, x + COL_W - 5, y);
    } else {
      ctx.textAlign = "left";
      ctx.fillText(it.t, x + 5, y);
    }
    ctx.restore();
  }

  // Range border + focus-cell border.
  ctx.strokeStyle = colors.accent;
  ctx.lineWidth = 2;
  if (selX + selW > HW && selY + selH > HH) {
    ctx.strokeRect(Math.max(HW, selX) + 1, Math.max(HH, selY) + 1, selW - 1, selH - 1);
  }
  if (sx >= HW && sy >= HH) {
    ctx.strokeRect(sx + 1, sy + 1, COL_W - 1, ROW_H - 1);
  }

  cellRef.textContent = colName(state.sel.col) + (state.sel.row + 1);
  if (wasm) refreshFormulaBar();
}

function refreshFormulaBar() {
  if (state.editing) return;
  fInput.value = wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  document.getElementById("tb-undo").disabled = !wasm.session_can_undo();
  document.getElementById("tb-redo").disabled = !wasm.session_can_redo();
  const s = selRect();
  document
    .getElementById("tb-bold")
    .setAttribute("aria-pressed", wasm.session_range_bold(state.sheet, s.r0, s.c0, s.r1, s.c1) ? "true" : "false");
}

function cellAt(px, py) {
  if (px < HW || py < HH) return null;
  return {
    col: state.firstCol + Math.floor((px - HW) / COL_W),
    row: state.firstRow + Math.floor((py - HH) / ROW_H),
  };
}

function select(row, col) {
  const r = Math.max(0, row);
  const c = Math.max(0, col);
  state.sel = { row: r, col: c };
  state.anchor = { row: r, col: c };
  ensureVisible();
  draw();
}

// Extend the selection to (row, col), keeping the anchor.
function extend(row, col) {
  state.sel = { row: Math.max(0, row), col: Math.max(0, col) };
  ensureVisible();
  draw();
}

function ensureVisible() {
  const v = visible();
  const viewRows = Math.floor((v.h - HH) / ROW_H);
  const viewCols = Math.floor((v.w - HW) / COL_W);
  if (state.sel.row < state.firstRow) state.firstRow = state.sel.row;
  if (state.sel.row >= state.firstRow + viewRows) state.firstRow = state.sel.row - viewRows + 1;
  if (state.sel.col < state.firstCol) state.firstCol = state.sel.col;
  if (state.sel.col >= state.firstCol + viewCols) state.firstCol = state.sel.col - viewCols + 1;
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

function toggleBold() {
  const s = selRect();
  try { wasm.session_toggle_bold(state.sheet, s.r0, s.c0, s.r1, s.c1); }
  catch (e) { status.textContent = `error: ${e}`; }
  draw();
}
function setFill(hex) {
  const s = selRect();
  try { wasm.session_set_fill(state.sheet, s.r0, s.c0, s.r1, s.c1, hex); }
  catch (e) { status.textContent = `error: ${e}`; }
  draw();
}
function clearSelection() {
  const s = selRect();
  try { wasm.session_clear_range(state.sheet, s.r0, s.c0, s.r1, s.c1); } catch {}
  draw();
}
function doUndo() { try { wasm.session_undo(); } catch {} draw(); }
function doRedo() { try { wasm.session_redo(); } catch {} draw(); }
function doSave() {
  const bytes = wasm.session_save();
  const blob = new Blob([bytes], { type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "opencalc.xlsx";
  a.click();
}
async function doCopy() {
  const s = selRect();
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
  const x = HW + (state.sel.col - state.firstCol) * COL_W;
  const y = HH + (state.sel.row - state.firstRow) * ROW_H;
  inline.style.display = "block";
  inline.style.left = x + "px";
  inline.style.top = y + "px";
  inline.style.width = COL_W + "px";
  inline.style.height = ROW_H + "px";
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

function wireEvents() {
  canvas.addEventListener("mousedown", (e) => {
    const rect = canvas.getBoundingClientRect();
    const hit = cellAt(e.clientX - rect.left, e.clientY - rect.top);
    if (hit) {
      endInline();
      if (e.shiftKey) extend(hit.row, hit.col);
      else { select(hit.row, hit.col); state.dragging = true; }
      canvas.focus();
    }
  });
  canvas.addEventListener("mousemove", (e) => {
    if (!state.dragging) return;
    const rect = canvas.getBoundingClientRect();
    const hit = cellAt(e.clientX - rect.left, e.clientY - rect.top);
    if (hit && (hit.row !== state.sel.row || hit.col !== state.sel.col)) extend(hit.row, hit.col);
  });
  window.addEventListener("mouseup", () => { state.dragging = false; });
  canvas.addEventListener("dblclick", (e) => {
    const rect = canvas.getBoundingClientRect();
    const hit = cellAt(e.clientX - rect.left, e.clientY - rect.top);
    if (hit) {
      select(hit.row, hit.col);
      startInline();
    }
  });
  let accY = 0;
  let accX = 0;
  wrap.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      // Normalize line/page wheel modes to pixels, accumulate, and step one
      // row/column per row-height/col-width so scrolling is smooth & proportional.
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? wrap.clientHeight : 1;
      accY += e.deltaY * unit * scrollDamp;
      accX += e.deltaX * unit * scrollDamp;
      let changed = false;
      while (accY >= ROW_H) { state.firstRow += 1; accY -= ROW_H; changed = true; }
      while (accY <= -ROW_H && state.firstRow > 0) { state.firstRow -= 1; accY += ROW_H; changed = true; }
      while (accX >= COL_W) { state.firstCol += 1; accX -= COL_W; changed = true; }
      while (accX <= -COL_W && state.firstCol > 0) { state.firstCol -= 1; accX += COL_W; changed = true; }
      // Only discard over-scroll past the top/left edge; keep normal accumulation.
      if (state.firstRow === 0 && accY < 0) accY = 0;
      if (state.firstCol === 0 && accX < 0) accX = 0;
      if (changed) draw();
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

  document.getElementById("tb-new").addEventListener("click", () => { wasm.session_new(); seed(); });
  document.getElementById("tb-save").addEventListener("click", doSave);
  document.getElementById("tb-bold").addEventListener("click", () => { toggleBold(); canvas.focus(); });
  for (const b of document.querySelectorAll("#tb-fill button")) {
    b.addEventListener("click", () => { setFill(b.dataset.c); canvas.focus(); });
  }
  document.getElementById("tb-open").addEventListener("change", async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    try { wasm.session_open(bytes); status.textContent = "opened " + file.name; }
    catch (err) { status.textContent = `error: ${err}`; }
    state.firstRow = state.firstCol = 0;
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

  // Restore saved preferences (default scroll speed is 0.40).
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
  resize();
  status.textContent = `engine v${wasm.version()}`;
}

main().catch((err) => { status.textContent = `failed: ${err}`; });
