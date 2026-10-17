// The renderer benchmark (`PERF-18`): does the canvas grid miss 60 fps, and if
// it does, is the time in raster (a GPU renderer would help) or in getting and
// laying out the data (it would not)?
//
// **Not a gate, and not part of the browser-smoke run.** A wall-clock number on
// a shared machine is not something to fail a build on (`PERF-15`). This is the
// instrument for the decision `PERF-18` defers — "build only when a benchmark
// shows the canvas renderer missing 60 fps; measure first" — and it is in the
// tree so the next person to ask that question gets the same answer for the
// same build. Results and their reading are in
// `docs/101-RENDER-BENCHMARK.md`.
//
// # What it measures, and why each number is there
//
// `frame-profile.mjs` learned the hard way that the interval between animation
// frames is blind to any cost that fits under vsync (`PERF-13`). So every
// scenario reports two families, and neither stands in for the other:
//
//   - **interval** — milliseconds between `requestAnimationFrame` ticks while
//     the grid moves. What a user sees. Floored at the display period, so read
//     its tail: `>20ms` is a missed vsync on a 60 Hz panel.
//   - **draw** — milliseconds the editor's own `draw()` took, per frame, split
//     into its phases. Recorded by the editor itself when the page is loaded
//     with `?perfmarks=1` (the probe is a constant branch otherwise; see
//     `editor.perf-marks.spec.mjs`). This is the main-thread cost, and it is
//     what is compared against the 16.7 ms frame and docs/30's 8 ms budget.
//
// The phases follow `draw()`'s structure, grouped for the one question that
// matters here:
//
//   data   = measure + fetch + spill + autoheight — geometry and the visible
//            window's cells out of the engine, plus the text measuring
//            auto-height needs. A GPU renderer still has to do every bit of it.
//   paint  = grid + objects + fills + text + borders + merges + overlays +
//            headers — issuing canvas work. In Chrome this *records* commands;
//            the raster happens later on the GPU process. A GPU renderer would
//            replace this, but so would cheaper canvas use.
//   dom    = the per-frame DOM and state upkeep after the paint (formula bar,
//            name box, scrollbars, a11y scheduling, host events).
//
// Raster itself is not on the main thread in an accelerated canvas, so `draw`
// cannot see it. `--trace` captures a Chrome trace of a scenario and reports
// busy time per thread — the renderer main thread against the GPU process and
// the compositor — which is the split that says whether raster is the limit.
//
// # How the grid is moved
//
// One synthetic `wheel` event on `#grid-wrap` per animation frame, dispatched
// from the recorder's own rAF callback. That exercises the editor's real
// handler (axis lock, damping, clamping, `scheduleDraw`), at a speed that is a
// property of the scenario rather than of how fast a CDP round trip happens to
// be. When the view hits an edge the direction reverses, so a scenario keeps
// moving for its whole length on any sheet size. `vertical-input` drives the
// same scroll through Playwright's real input pipeline instead, as a check that
// the synthetic path is not flattering the result.
//
// # Running it
//
//     cd tests/browser
//     node render-bench.mjs                      # everything, 3 runs
//     node render-bench.mjs --runs=1 --only=big-vertical
//     node render-bench.mjs --trace=big-vertical@3840x2160@2x@10%
//     node render-bench.mjs --gpu=off            # software canvas, for contrast
//     node render-bench.mjs --json=out.json      # raw samples, for the report
//
// Needs a current `webapp/pkg` (`python3 tools/build-wasm.py`). Starts its own
// `serve.py` on a fixed port of its own (`OPENCALC_BENCH_PORT`, default 8791 —
// not an ephemeral one, which a long run on a shared machine can exhaust) and
// checks that it serves this checkout (`CI-025`).
//
// **Never opens a window.** The full Chromium binary runs in `--headless=new`
// with the GPU flags (`--use-angle=metal` on macOS), which — unlike the old
// headless shell, which rasterises with SwiftShader — gets the real GPU. The
// run does not assume it: it prints what it got, from `chrome://gpu` and
// `WEBGL_debug_renderer_info`, and labels a run without hardware raster as
// software raster. That line belongs next to any number quoted from it. Close other heavy work first — the run prints the load
// average before and after, and a number taken at a load above the core count
// is a number about the machine.
import { chromium } from "@playwright/test";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { realpathSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const WEBAPP = path.resolve(HERE, "../../webapp");

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Nearest-rank quantiles: interpolating between two samples of a clamped clock
/// invents precision the clock does not have.
export function quantiles(samples) {
  const s = [...samples].sort((a, b) => a - b);
  if (!s.length) return { n: 0, p50: NaN, p95: NaN, p99: NaN, max: NaN };
  const at = (q) => s[Math.min(s.length - 1, Math.floor(s.length * q))];
  return { n: s.length, p50: at(0.5), p95: at(0.95), p99: at(0.99), max: s[s.length - 1] };
}

const share = (xs, over) => (xs.length ? (100 * xs.filter((x) => x > over).length) / xs.length : 0);
const f1 = (n) => (Number.isFinite(n) ? n.toFixed(1) : "-");

// ---------------------------------------------------------------------------
// Workbooks, built by the engine in the page and saved to bytes once
// ---------------------------------------------------------------------------

/// The 1M-cell sheet: 20,000 rows x 50 columns, every cell populated.
///
/// Mixed the way a real export is: integers, decimals, currency, percentages,
/// dates, booleans, short labels and long text, a formula column, a header row
/// in bold, banded fills, a bordered block and three conditional formats (a
/// threshold fill, a data bar, a colour scale). 50 columns so the sheet is
/// wider than a 1920 px window and a horizontal scroll has somewhere to go.
export function buildBigWorkbook({ rows, cols, cf = true }) {
  const ed = window.opencalcEditor;
  const api = ed.wasmApi();
  const words = ["North", "South", "East", "West", "Central", "Pacific", "Atlantic", "Mountain"];
  const header = [];
  for (let c = 0; c < cols; c += 1) header.push(`Field ${c + 1}`);
  api.session_paste_tsv(0, 0, 0, header.join("\t"));
  const CHUNK = 1000;
  for (let r0 = 1; r0 < rows; r0 += CHUNK) {
    const lines = [];
    for (let r = r0; r < Math.min(rows, r0 + CHUNK); r += 1) {
      const line = [];
      for (let c = 0; c < cols; c += 1) {
        const k = c % 10;
        if (c === cols - 1) line.push(`=A${r + 1}*2+B${r + 1}`);
        else if (k === 0) line.push(String((r * 7919 + c * 104729) % 100000));
        else if (k === 1) line.push(String(((r * 31 + c) % 10000) / 7));
        else if (k === 2) line.push(`${words[(r + c) % words.length]} ${r}`);
        else if (k === 3) line.push(String(((r * 13 + c) % 5000) * 1.25));
        else if (k === 4) line.push(String(((r + c) % 100) / 100));
        else if (k === 5) line.push(String(40000 + ((r + c) % 3000)));
        else if (k === 6) line.push(`${words[r % words.length]} region quarterly summary line ${r}`);
        else if (k === 7) line.push(String(((r * 17 + c) % 2000) - 1000));
        else if (k === 8) line.push(r % 3 === 0 ? "TRUE" : "FALSE");
        else line.push(`SKU-${(r * c) % 99991}`);
      }
      lines.push(line.join("\t"));
    }
    api.session_paste_tsv(0, r0, 0, lines.join("\n"));
  }
  const last = rows - 1;
  for (let c = 0; c < cols; c += 1) {
    const k = c % 10;
    const code = k === 1 ? "#,##0.00" : k === 3 ? "$#,##0.00" : k === 4 ? "0.0%"
      : k === 5 ? "yyyy-mm-dd" : k === 7 ? "#,##0;[Red]-#,##0" : null;
    if (code) api.session_set_number_format(0, 1, c, last, c, code);
  }
  api.session_set_font_flags(0, 0, 0, 0, cols - 1, true, false, false, false);
  api.session_set_fill(0, 0, 0, 0, cols - 1, "#D9E1F2", -1, 0);
  // Banded fills: three rows in ten, across the sheet.
  for (let r = 1; r < rows; r += 10) api.session_set_fill(0, r, 0, Math.min(last, r + 2), cols - 1, "#F2F2F2", -1, 0);
  // A solid block of fills and a bordered block, a few columns wide.
  api.session_set_fill(0, 1, 10, last, 12, "#FFF2CC", -1, 0);
  api.session_set_border(0, 1, 20, last, 29, "all", "thin", "#808080");
  if (cf) {
    api.session_add_cf(0, 1, 0, last, 0, "gt", 80000, 0, "", "#F8CBAD");
    api.session_add_cf(0, 1, 3, last, 3, "databar", 0, 0, "#5B9BD5", "");
    api.session_add_cf(0, 1, 4, last, 4, "colorscale", 0, 0, "#F8696B,#FFEB84,#63BE7B", "");
  }
  return api.session_used_bounds ? api.session_used_bounds(0) : null;
}

/// The layout-heavy sheet: many merges, wrapped paragraphs, rotated and
/// stacked text — the per-cell paths that do the most work per cell.
///
/// 20,000 rows x 12 columns. 10,000 merged blocks (two per four rows), a
/// wrapped long-text column whose rows auto-grow, text at 45 and 90 degrees,
/// and a stacked column.
export function buildLayoutWorkbook({ rows }) {
  const api = window.opencalcEditor.wasmApi();
  const CHUNK = 1000;
  for (let r0 = 0; r0 < rows; r0 += CHUNK) {
    const lines = [];
    for (let r = r0; r < Math.min(rows, r0 + CHUNK); r += 1) {
      lines.push([
        `Group ${r >> 2}`, "", `This is a longer note for row ${r} that wraps onto several lines inside its cell`,
        `Rot ${r}`, String(r * 3), String(r % 97), "", `Up ${r}`, `S${r % 100}`, String(r / 3), `Item ${r}`, String(r),
      ].join("\t"));
    }
    api.session_paste_tsv(0, r0, 0, lines.join("\n"));
  }
  const last = rows - 1;
  api.session_toggle_wrap(0, 0, 2, last, 2);
  api.session_set_rotation(0, 0, 3, last, 3, 45);
  api.session_set_rotation(0, 0, 7, last, 7, 90);
  api.session_set_rotation(0, 0, 8, last, 8, 255);
  // **The undo history is cleared as it goes, and that is not optional.** A
  // merge is a whole-sheet metadata edit: each one copies every merge already
  // on the sheet into the operation and its inverse, so ten thousand of them
  // through one history is quadratic in memory and ran a renderer at 100% CPU
  // for twenty minutes without finishing. Clearing every hundred keeps it
  // linear; the workbook that comes out is the same.
  let merges = 0;
  for (let r = 0; r + 3 < rows; r += 4) {
    api.session_merge_cells(0, r, 0, r + 1, 1); merges += 1;
    api.session_merge_cells(0, r + 2, 5, r + 3, 6); merges += 1;
    if (merges % 100 === 0) api.session_clear_history();
  }
  api.session_clear_history();
  return merges;
}

/// The isolation books: the same content as `big` with one thing changed, so a
/// difference between two runs has one cause. `plain` drops the conditional
/// formats; `small` is a tenth of the rows. If a frame cost moves with either,
/// it is not a rendering cost — the visible window is identical in all three.
const BOOKS = {
  big: { rows: 20000, cols: 50, cf: true },
  plain: { rows: 20000, cols: 50, cf: false },
  small: { rows: 2000, cols: 50, cf: true },
};

async function buildBytes(browser, base, which) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await page.goto(`${base}/editor.html`);
  await waitForEngine(page);
  const t0 = Date.now();
  const built = which === "layout"
    ? await page.evaluate(buildLayoutWorkbook, { rows: 20000 })
    : await page.evaluate(buildBigWorkbook, BOOKS[which]);
  const b64 = await page.evaluate(() => {
    const bytes = window.opencalcEditor.wasmApi().session_save();
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  });
  await context.close();
  const buf = Buffer.from(b64, "base64");
  console.log(`built ${which}: ${buf.length} bytes in ${((Date.now() - t0) / 1000).toFixed(1)}s (${JSON.stringify(built)})`);
  return buf;
}

// ---------------------------------------------------------------------------
// In-page recorder
//
// Shipped to the page as source: closes over nothing from this module.
// ---------------------------------------------------------------------------

/// Move the grid for `frames` animation frames and record what each cost.
///
/// `motion`: `{ dx, dy }` px per frame, or `{ fling: { v0, decay } }` for a
/// thrown scroll that decays and is re-thrown when it has stopped, or
/// `{ idle: true }` to record without moving (used while the window is resized
/// from outside). Reverses at an edge so the view keeps moving.
export function recordFrames({ motion, frames, warmup, maxMs = 20_000 }) {
  const ed = window.opencalcEditor;
  const wrap = document.getElementById("grid-wrap");
  const r = wrap.getBoundingClientRect();
  const at = { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 };
  const longtasks = [];
  const loafs = [];
  const obs = [];
  const watch = (type, sink) => {
    try {
      const o = new PerformanceObserver((list) => { for (const e of list.getEntries()) sink.push(e); });
      o.observe({ type, buffered: false });
      obs.push(o);
    } catch { /* not supported by this build */ }
  };
  let sx = 1;
  let sy = 1;
  let v = motion.fling ? motion.fling.v0 : 0;
  const step = () => {
    if (motion.idle) return;
    let dx;
    let dy;
    if (motion.fling) {
      if (Math.abs(v) < 2) v = motion.fling.v0;
      dx = 0;
      dy = v;
      v *= motion.fling.decay;
    } else {
      dx = motion.dx;
      dy = motion.dy;
    }
    const before = { x: ed.state.scrollX, y: ed.state.scrollY };
    wrap.dispatchEvent(new WheelEvent("wheel", {
      deltaX: dx * sx, deltaY: dy * sy, deltaMode: 0, bubbles: true, cancelable: true, ...at,
    }));
    // An edge: the offset did not move on an axis that was asked to.
    if (dy && ed.state.scrollY === before.y) sy = -sy;
    if (dx && ed.state.scrollX === before.x) sx = -sx;
  };
  return new Promise((resolve) => {
    let n = 0;
    let last = 0;
    let began = 0;
    let recording = 0;
    const intervals = [];
    const tick = (now) => {
      if (!began) began = now;
      // A wall-clock cap on both halves, so a scenario that runs at a frame a
      // second ends in seconds rather than hours. The report says how many
      // frames it got; a short count is itself the finding.
      if (n < warmup && now - began > maxMs / 4) warmup = n;
      if (n === warmup) {
        recording = now;
        ed.takePerfFramesForTest();
        performance.clearMeasures();
        watch("longtask", longtasks);
        watch("long-animation-frame", loafs);
      }
      if (n > warmup) intervals.push(now - last);
      last = now;
      n += 1;
      if (n > warmup + frames || (recording && n > warmup + 2 && now - recording > maxMs)) {
        for (const o of obs) o.disconnect();
        const draws = ed.takePerfFramesForTest();
        resolve({
          intervals,
          draws,
          longtasks: longtasks.map((e) => e.duration),
          loafs: loafs.map((e) => ({
            duration: e.duration,
            render: e.renderStart ? e.duration - (e.renderStart - e.startTime) : 0,
            scripts: (e.scripts || []).map((s) => ({ d: s.duration, src: `${s.sourceFunctionName || ""}@${(s.sourceURL || "").split("/").pop()}:${s.sourceCharPosition}` })),
          })),
        });
        return;
      }
      step();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/// Set the zoom. 25% through the editor's own `setZoom`; below its 25% floor
/// (Excel's is 10%) by doing exactly what `setZoom` does without the clamp, so
/// the very-zoomed-out case can be measured before the editor allows it.
export function applyZoom(z) {
  const ed = window.opencalcEditor;
  if (z >= ed.ZOOM_MIN) {
    ed.setZoomForTest(z);
  } else {
    ed.state.zoom = z;
    ed.clearHeightMemo();
    ed.invalidateGrowth();
    ed.resize();
  }
  ed.state.scrollX = 0;
  ed.state.scrollY = 0;
  ed.draw();
  return ed.state.zoom;
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

const VIEWPORTS = [
  { name: "1920x1080@1x", width: 1920, height: 1080, dsf: 1 },
  { name: "1920x1080@2x", width: 1920, height: 1080, dsf: 2 },
  { name: "3840x2160@1x", width: 3840, height: 2160, dsf: 1 },
  { name: "3840x2160@2x", width: 3840, height: 2160, dsf: 2 },
];

const MOTIONS = {
  vertical: { dx: 0, dy: 120 },
  horizontal: { dx: 90, dy: 0 },
  diagonal: { dx: 60, dy: 120 },
  // A trackpad throw: 2,400 px in the first frame, decaying 8% a frame — about
  // a screenful and a half a frame at the start, which is a hard flick.
  fling: { fling: { v0: 2400, decay: 0.92 } },
};

/// Every scenario, as `{ id, book, vp, zoom, kind, motion? }`. The id is what
/// `--only` and `--trace` match against.
function scenarioList() {
  const out = [];
  const add = (book, kind, vp, zoom, extra = {}) => out.push({
    id: `${book}-${kind}@${vp.name}@${Math.round(zoom * 100)}%`, book, kind, vp, zoom, ...extra,
  });
  for (const vp of VIEWPORTS) {
    for (const kind of ["vertical", "horizontal", "diagonal", "fling"]) add("big", kind, vp, 1, { motion: MOTIONS[kind] });
    for (const zoom of [0.25, 0.1]) {
      for (const kind of ["vertical", "diagonal"]) add("big", kind, vp, zoom, { motion: MOTIONS[kind] });
    }
  }
  for (const vp of [VIEWPORTS[0], VIEWPORTS[3]]) {
    add("big", "resize", vp, 1);
    add("big", "charts", vp, 1, { motion: MOTIONS.vertical });
  }
  add("big", "vertical-input", VIEWPORTS[0], 1);
  for (const vp of [VIEWPORTS[0], VIEWPORTS[3]]) {
    add("plain", "vertical", vp, 1, { motion: MOTIONS.vertical });
    add("small", "vertical", vp, 1, { motion: MOTIONS.vertical });
  }
  for (const vp of [VIEWPORTS[0], VIEWPORTS[3]]) {
    for (const kind of ["vertical", "diagonal", "fling"]) add("layout", kind, vp, 1, { motion: MOTIONS[kind] });
    add("layout", "vertical", vp, 0.25, { motion: MOTIONS.vertical });
  }
  return out;
}

// ---------------------------------------------------------------------------
// Driving one scenario
// ---------------------------------------------------------------------------

async function waitForEngine(page) {
  await page.waitForFunction(() => !!document.querySelector("#tb-status")?.dataset?.engine, null, { timeout: 60_000 });
}

/// A booted editor at `dsf`, holding `bytes` as its document.
async function openEditor(browser, base, dsf, bytes) {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: dsf });
  const page = await context.newPage();
  await page.route("**/__bench/book.xlsx", (route) => route.fulfill({
    status: 200, contentType: "application/octet-stream", body: bytes,
  }));
  await page.goto(`${base}/editor.html?perfmarks=1`);
  await waitForEngine(page);
  const opened = await page.evaluate(async () => {
    const buf = new Uint8Array(await (await fetch("/__bench/book.xlsx")).arrayBuffer());
    const t0 = performance.now();
    const ok = window.opencalcEditor.openBytes(buf, "bench.xlsx", -1);
    return { ok, ms: performance.now() - t0 };
  });
  if (!opened.ok) throw new Error("the benchmark workbook did not open");
  return { context, page, openMs: opened.ms };
}

const FRAMES = 180; // three seconds at 60 Hz
const WARMUP = 30;

async function runScenario(page, sc) {
  const load = os.loadavg()[0];
  await page.setViewportSize({ width: sc.vp.width, height: sc.vp.height });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const zoom = await page.evaluate(applyZoom, sc.zoom);
  if (Math.abs(zoom - sc.zoom) > 1e-9) throw new Error(`${sc.id}: zoom is ${zoom}, wanted ${sc.zoom}`);
  let charts = 0;
  if (sc.kind === "charts") {
    charts = await page.evaluate(() => {
      const api = window.opencalcEditor.wasmApi();
      // Four charts over the first rows' data, the view scrolled through them.
      if (JSON.parse(api.session_chart_defs(0)).length === 0) {
        api.session_create_chart(0, 0, 0, 40, 4, "column");
        api.session_create_chart(0, 0, 0, 200, 1, "line");
        api.session_create_chart(0, 0, 3, 60, 4, "bar");
        api.session_create_chart(0, 0, 0, 12, 1, "pie");
      }
      window.opencalcEditor.draw();
      return JSON.parse(api.session_chart_defs(0)).length;
    });
    if (charts < 4) throw new Error(`${sc.id}: built ${charts} charts`);
  }
  const window_ = await page.evaluate(() => {
    const g = window.opencalcEditor.frameWindowForTest();
    return `${g.colIdx}x${g.rowIdx}`;
  });

  let rec;
  if (sc.kind === "resize") {
    // The window resized from outside while the recorder watches. Two sizes a
    // fifth apart, alternated as fast as the emulation round trip allows.
    const recording = page.evaluate(recordFrames, { motion: { idle: true }, frames: FRAMES, warmup: WARMUP });
    const small = { width: Math.round(sc.vp.width * 0.8), height: Math.round(sc.vp.height * 0.8) };
    let flip = false;
    let done = false;
    recording.then(() => { done = true; });
    while (!done) {
      flip = !flip;
      await page.setViewportSize(flip ? small : { width: sc.vp.width, height: sc.vp.height });
      await page.waitForTimeout(30);
    }
    rec = await recording;
  } else if (sc.kind === "vertical-input") {
    const box = await page.locator("#grid-wrap").boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    const recording = page.evaluate(recordFrames, { motion: { idle: true }, frames: FRAMES, warmup: WARMUP });
    let done = false;
    recording.then(() => { done = true; });
    let sign = 1;
    let i = 0;
    while (!done) {
      i += 1;
      if (i % 150 === 0) sign = -sign;
      await page.mouse.wheel(0, 120 * sign);
    }
    rec = await recording;
  } else {
    rec = await page.evaluate(recordFrames, { motion: sc.motion, frames: FRAMES, warmup: WARMUP });
  }
  // Leave the next scenario at the top-left at 100%.
  await page.evaluate(applyZoom, 1);
  return { ...rec, window: window_, charts, load };
}

// ---------------------------------------------------------------------------
// Summaries
// ---------------------------------------------------------------------------

const DATA = ["measure", "fetch", "spill", "autoheight"];
const PAINT = ["grid", "objects", "fills", "text", "borders", "merges", "overlays", "headers"];

function phaseMeans(draws) {
  const keys = [...DATA, ...PAINT, "dom", "total"];
  const out = {};
  for (const k of keys) out[k] = draws.length ? draws.reduce((a, d) => a + (d[k] || 0), 0) / draws.length : 0;
  out.data = DATA.reduce((a, k) => a + out[k], 0);
  out.paint = PAINT.reduce((a, k) => a + out[k], 0);
  out.cells = draws.length ? Math.round(draws.reduce((a, d) => a + d.cells, 0) / draws.length) : 0;
  return out;
}

export function summarise(runs) {
  const intervals = runs.flatMap((r) => r.intervals);
  const draws = runs.flatMap((r) => r.draws);
  const totals = draws.map((d) => d.total);
  return {
    runs: runs.length,
    interval: { ...quantiles(intervals), over16_7: share(intervals, 16.7), over20: share(intervals, 20), over33: share(intervals, 33.4) },
    draw: { ...quantiles(totals), over8: share(totals, 8), over16_7: share(totals, 16.7) },
    drawsPerFrame: intervals.length ? draws.length / intervals.length : 0,
    phases: phaseMeans(draws),
    longtasks: runs.reduce((a, r) => a + r.longtasks.length, 0),
    longtaskMax: Math.max(0, ...runs.flatMap((r) => r.longtasks)),
    perRunP50: runs.map((r) => quantiles(r.draws.map((d) => d.total)).p50),
    window: runs[0]?.window,
    load: runs.map((r) => Number(r.load.toFixed(1))),
  };
}

function printRow(id, s) {
  const p = s.phases;
  console.log(
    `${id.padEnd(40)} interval p50 ${f1(s.interval.p50)} p95 ${f1(s.interval.p95)} p99 ${f1(s.interval.p99)} `
    + `>16.7 ${f1(s.interval.over16_7)}% >20 ${f1(s.interval.over20)}% | draw p50 ${f1(s.draw.p50)} `
    + `p95 ${f1(s.draw.p95)} p99 ${f1(s.draw.p99)} >16.7 ${f1(s.draw.over16_7)}% | data ${f1(p.data)} `
    + `paint ${f1(p.paint)} dom ${f1(p.dom)} cells ${p.cells} | n ${s.interval.n} lt ${s.longtasks} load ${s.load.join("/")}`,
  );
}

// ---------------------------------------------------------------------------
// Tracing: where the time goes, by thread
// ---------------------------------------------------------------------------

const TRACE_CATEGORIES = [
  "toplevel", "devtools.timeline", "disabled-by-default-devtools.timeline",
  "disabled-by-default-devtools.timeline.frame", "blink.user_timing", "gpu", "viz", "cc", "v8.execute",
].join(",");

/// Busy time per thread over the traced window, from a Chrome trace.
///
/// "Busy" is the union of the thread's `toplevel` task slices — the scheduler's
/// own task boundaries, so nested events are not counted twice. Reported per
/// process/thread name, plus the main thread's split by event name.
export function analyseTrace(trace) {
  const events = trace.traceEvents || trace;
  const names = new Map();
  const procs = new Map();
  for (const e of events) {
    if (e.ph !== "M") continue;
    if (e.name === "thread_name") names.set(`${e.pid}:${e.tid}`, e.args.name);
    if (e.name === "process_name") procs.set(e.pid, e.args.name);
  }
  const draws = events.filter((e) => e.name === "oc:draw" && (e.ph === "b" || e.ph === "e" || e.ph === "X"));
  const ts = events.filter((e) => e.ts && e.ph === "X").map((e) => e.ts);
  const t0 = Math.min(...ts);
  const t1 = Math.max(...ts.map((t, i) => t));
  const busy = new Map();
  const byName = new Map();
  for (const e of events) {
    if (e.ph !== "X" || !e.dur) continue;
    const key = `${procs.get(e.pid) || e.pid} / ${names.get(`${e.pid}:${e.tid}`) || e.tid}`;
    if (String(e.cat).includes("toplevel")) {
      if (!busy.has(key)) busy.set(key, []);
      busy.get(key).push([e.ts, e.ts + e.dur]);
    }
    if ((names.get(`${e.pid}:${e.tid}`) || "") === "CrRendererMain" && !String(e.cat).includes("toplevel")) {
      byName.set(e.name, (byName.get(e.name) || 0) + e.dur);
    }
  }
  const union = (iv) => {
    iv.sort((a, b) => a[0] - b[0]);
    let total = 0;
    let [s, en] = iv[0];
    for (const [a, b] of iv.slice(1)) {
      if (a > en) { total += en - s; s = a; en = b; } else en = Math.max(en, b);
    }
    return total + (en - s);
  };
  const threads = [...busy.entries()].map(([k, iv]) => ({ thread: k, busyMs: union(iv) / 1000 }))
    .sort((a, b) => b.busyMs - a.busyMs);
  const main = [...byName.entries()].map(([k, us]) => ({ event: k, ms: us / 1000 }))
    .sort((a, b) => b.ms - a.ms).slice(0, 25);
  return { spanMs: (t1 - t0) / 1000, draws: draws.length, threads, main };
}

/// Self and total time per function from a V8 CPU profile, top `n` by self.
///
/// Total (self plus callees) is what names a culprit: `draw` is not slow, a
/// function it calls is, and the self column alone would credit the wasm
/// export or the JSON parser it lands in without saying who asked.
export function analyseProfile(profile, n = 30) {
  const byId = new Map(profile.nodes.map((nd) => [nd.id, nd]));
  const parent = new Map();
  for (const nd of profile.nodes) for (const c of nd.children || []) parent.set(c, nd.id);
  const dt = new Map();
  for (let i = 0; i < profile.samples.length; i += 1) {
    const id = profile.samples[i];
    dt.set(id, (dt.get(id) || 0) + (profile.timeDeltas[i] || 0));
  }
  const key = (nd) => {
    const cf = nd.callFrame;
    const file = (cf.url || "").split("/").pop().split("?")[0];
    return `${cf.functionName || "(anonymous)"} ${file}${file ? `:${cf.lineNumber + 1}` : ""}`;
  };
  const self = new Map();
  const total = new Map();
  let all = 0;
  for (const [id, us] of dt) {
    all += us;
    const nd = byId.get(id);
    self.set(key(nd), (self.get(key(nd)) || 0) + us);
    const seen = new Set();
    for (let cur = id; cur !== undefined; cur = parent.get(cur)) {
      const k = key(byId.get(cur));
      if (seen.has(k)) continue;
      seen.add(k);
      total.set(k, (total.get(k) || 0) + us);
    }
  }
  const rows = (m) => [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, n)
    .map(([k, us]) => ({ fn: k, ms: us / 1000, pct: (100 * us) / all }));
  return { sampledMs: all / 1000, self: rows(self), total: rows(total) };
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

async function startServer() {
  return new Promise((resolve, reject) => {
    const port = String(Number(process.env.OPENCALC_BENCH_PORT ?? 8791));
    const child = spawn("python3", [path.join(WEBAPP, "serve.py"), port], { stdio: ["ignore", "pipe", "pipe"] });
    let out = "";
    child.stdout.on("data", (d) => {
      out += d;
      const m = out.match(/localhost:(\d+)/);
      if (m) resolve({ child, port: Number(m[1]) });
    });
    child.on("exit", (code) => reject(new Error(`serve.py exited ${code}: ${out}`)));
    setTimeout(() => reject(new Error(`serve.py did not start: ${out}`)), 15_000);
  });
}

/// `CI-025`: prove the server is this checkout's before measuring anything.
///
/// Not `frame-profile.mjs`'s `assertServingThisCheckout`: that compares
/// `editor.core.js` byte for byte, and `serve.py` now rewrites module
/// specifiers with a build stamp, so the check refuses this checkout's own
/// server. `serve.py`'s identity endpoint names its root and pid instead, and
/// the wasm — which it does not rewrite, and which is the input most likely to
/// be stale — is still compared by content.
async function assertOurServer(server) {
  const who = await (await fetch(`http://127.0.0.1:${server.port}/__opencalc__`)).json();
  if (who.pid !== server.child.pid || realpathSync(who.root) !== realpathSync(WEBAPP)) {
    throw new Error(`the server on ${server.port} is not this run's: ${JSON.stringify(who)}`);
  }
  const rel = "pkg/casual_calc_wasm_bg.wasm";
  const sha = (b) => createHash("sha256").update(b).digest("hex").slice(0, 12);
  const disk = await readFile(path.join(WEBAPP, rel));
  const wire = Buffer.from(await (await fetch(`http://127.0.0.1:${server.port}/${rel}`)).arrayBuffer());
  if (sha(disk) !== sha(wire)) throw new Error(`/${rel} differs on the wire (${sha(wire)}) from disk (${sha(disk)})`);
  return `root ${who.root}, pid ${who.pid}, wasm ${sha(disk)}`;
}

async function gpuInfo(browser, base) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const info = { features: {}, webgl: null, version: browser.version() };
  try {
    await page.goto("chrome://gpu");
    await page.waitForTimeout(1500);
    // `chrome://gpu` renders inside a custom element's shadow root; a
    // host's `innerText` is the flattened, laid-out text, shadow included.
    const text = await page.evaluate(() => {
      const root = document.querySelector("info-view")?.shadowRoot;
      return root ? [...root.querySelectorAll("li")].map((e) => e.innerText).join("\n") : document.body.innerText;
    });
    for (const line of text.split("\n")) {
      const m = line.match(/^\s*(Canvas|Compositing|Rasterization|WebGL|WebGPU|Skia Graphite)\s*:\s*(.+)$/);
      if (m) info.features[m[1]] = m[2].trim();
    }
  } catch (e) {
    info.features.error = String(e.message || e);
  }
  try {
    await page.goto(`${base}/editor.html`);
    info.webgl = await page.evaluate(() => {
      const gl = document.createElement("canvas").getContext("webgl");
      if (!gl) return null;
      const ext = gl.getExtension("WEBGL_debug_renderer_info");
      return ext ? `${gl.getParameter(ext.UNMASKED_VENDOR_WEBGL)} | ${gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)}` : gl.getParameter(gl.RENDERER);
    });
    info.refreshHz = await page.evaluate(() => new Promise((res) => {
      const t = [];
      let last = 0;
      const tick = (now) => {
        if (last) t.push(now - last);
        last = now;
        if (t.length < 120) requestAnimationFrame(tick);
        else { t.sort((a, b) => a - b); res(Math.round(1000 / t[60])); }
      };
      requestAnimationFrame(tick);
    }));
  } finally {
    await context.close();
  }
  // The label every number from this run carries. Hardware only when both the
  // canvas and raster say so *and* WebGL is not SwiftShader.
  info.raster = /Hardware/.test(info.features.Canvas || "") && /Hardware/.test(info.features.Rasterization || "")
    && !/SwiftShader/i.test(info.webgl || "") ? "hardware raster" : "software raster";
  return info;
}

function machine() {
  const cpus = os.cpus();
  return {
    cpu: cpus[0]?.model, cores: cpus.length, memGB: Math.round(os.totalmem() / 2 ** 30),
    os: `${os.type()} ${os.release()}`, load: os.loadavg().map((x) => x.toFixed(1)).join(" "),
    node: process.version,
  };
}

// ---------------------------------------------------------------------------

async function main() {
  const arg = (name, dflt) => {
    const hit = process.argv.find((a) => a.startsWith(`--${name}=`));
    return hit === undefined ? dflt : hit.slice(name.length + 3);
  };
  const runs = Number(arg("runs", 3));
  const only = arg("only", "");
  // `--profile=<id>`: a V8 CPU profile of one scenario instead of a trace —
  // which *functions* the main thread spends its time in.
  const profileId = arg("profile", "");
  const traceId = arg("trace", "") || profileId;
  const profile = !!profileId;
  const gpu = arg("gpu", "on");
  const headless = true;
  const jsonOut = arg("json", "");

  const server = await startServer();
  const base = `http://127.0.0.1:${server.port}`;
  const result = { when: new Date().toISOString(), machine: machine(), gpuMode: gpu, headless, scenarios: {} };
  let browser;
  try {
    console.log(`serving this checkout on ${server.port}: ${await assertOurServer(server)}`);
    // `headless: false` plus `--headless=new` is how Playwright 1.48 runs the
    // *full* browser headless: its own `headless: true` picks the old headless
    // shell, which has no GPU on macOS. No window is created either way.
    const args = [
      "--headless=new",
      "--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding",
      "--disable-background-timer-throttling",
    ];
    if (gpu === "off") {
      args.push("--disable-gpu", "--disable-accelerated-2d-canvas", "--disable-gpu-compositing");
    } else {
      args.push("--enable-gpu", "--enable-gpu-rasterization", "--ignore-gpu-blocklist");
      if (process.platform === "darwin") args.push("--use-angle=metal");
    }
    browser = await chromium.launch({ headless: false, args });
    result.gpu = await gpuInfo(browser, base);
    console.log(`machine: ${JSON.stringify(result.machine)}`);
    console.log(`browser: ${JSON.stringify(result.gpu)}`);

    let list = scenarioList();
    if (traceId) list = list.filter((s) => s.id === traceId);
    else if (only) list = list.filter((s) => s.id.includes(only));
    if (!list.length) throw new Error(`no scenario matches ${traceId || only}`);

    const books = {};
    for (const b of new Set(list.map((s) => s.book))) books[b] = await buildBytes(browser, base, b);

    const samples = new Map();
    const reps = traceId ? 1 : runs;
    for (let run = 0; run < reps; run += 1) {
      for (const dsf of [1, 2]) {
        for (const book of Object.keys(books)) {
          const group = list.filter((s) => s.vp.dsf === dsf && s.book === book);
          if (!group.length) continue;
          const { context, page, openMs } = await openEditor(browser, base, dsf, books[book]);
          console.log(`run ${run + 1}/${reps} dsf ${dsf} ${book}: opened in ${openMs.toFixed(0)} ms, load ${os.loadavg()[0].toFixed(1)}`);
          try {
            for (const sc of group) {
              let cdp = null;
              if (traceId && profile) {
                cdp = await page.context().newCDPSession(page);
                await cdp.send("Profiler.enable");
                await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
                await cdp.send("Profiler.start");
              } else if (traceId) {
                await browser.startTracing(page, { categories: TRACE_CATEGORIES.split(",") });
              }
              const rec = await runScenario(page, sc);
              if (cdp) {
                const { profile: prof } = await cdp.send("Profiler.stop");
                const a = analyseProfile(prof);
                result.profile = { id: sc.id, ...a };
                console.log(`cpu profile: ${a.sampledMs.toFixed(0)} ms sampled`);
                console.log("  by total (self + callees):");
                for (const r of a.total) console.log(`    ${r.ms.toFixed(0).padStart(6)} ms ${r.pct.toFixed(1).padStart(5)}%  ${r.fn}`);
                console.log("  by self:");
                for (const r of a.self) console.log(`    ${r.ms.toFixed(0).padStart(6)} ms ${r.pct.toFixed(1).padStart(5)}%  ${r.fn}`);
              }
              if (traceId && !profile) {
                const buf = await browser.stopTracing();
                const trace = JSON.parse(buf.toString("utf8"));
                const file = path.join(os.tmpdir(), `render-bench-${Date.now()}.json`);
                await writeFile(file, buf);
                const a = analyseTrace(trace);
                result.trace = { id: sc.id, file, ...a, frames: rec.intervals.length };
                console.log(`trace written to ${file}`);
                console.log(`span ${a.spanMs.toFixed(0)} ms, ${rec.intervals.length} frames recorded`);
                for (const t of a.threads.slice(0, 14)) {
                  console.log(`  ${t.thread.padEnd(50)} busy ${t.busyMs.toFixed(0).padStart(6)} ms  (${(t.busyMs / a.spanMs * 100).toFixed(0)}%)`);
                }
                console.log("  renderer main thread, by event (inclusive, ms):");
                for (const m of a.main) console.log(`    ${m.event.padEnd(44)} ${m.ms.toFixed(0).padStart(6)}`);
              }
              if (!samples.has(sc.id)) samples.set(sc.id, []);
              samples.get(sc.id).push(rec);
              printRow(`  ${sc.id}`, summarise([rec]));
            }
          } finally {
            await context.close();
          }
        }
      }
    }

    console.log(`\n== summary over ${reps} run(s): ms; interval = between rAF ticks, draw = editor draw() ==`);
    for (const sc of list) {
      const s = summarise(samples.get(sc.id) || []);
      result.scenarios[sc.id] = s;
      printRow(sc.id, s);
    }
  } finally {
    result.loadAfter = os.loadavg().map((x) => x.toFixed(1)).join(" ");
    console.log(`load average after: ${result.loadAfter}`);
    if (browser) await browser.close();
    server.child.kill();
  }
  if (jsonOut) await writeFile(jsonOut, JSON.stringify(result, null, 1));
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) await main();
