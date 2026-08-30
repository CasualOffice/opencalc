// Frame timing for the grid. **Two modes, because "is it faster" is two
// questions, and this file could only answer one of them.**
//
// Not a gate — `editor.frame-budget.spec.mjs` holds the structural assertions,
// because a wall-clock assertion on a shared machine is a flaky test that ends
// up deleted. This is the harness for answering "is it faster", and it exists
// in the tree because every attempt at that question so far has measured the
// wrong thing:
//
//   - The original `PERF-D-01` probe awaited a frame per wheel event while
//     `scheduleDraw` coalesces onto its own rAF, so it inflated the baseline.
//   - The rAF version, first time round, awaited the timing promise *before*
//     starting the scroll — so it measured 1.5s of idle frames and reported a
//     flat 16.7ms both before and after the fix. It only discriminates because
//     the scroll now runs during the measurement.
//   - And then `PERF-13`: it reported **the same 16.6ms with 1,963,431 bytes
//     crossing the WASM boundary every frame as with 61**. `CHT-13` was an
//     8.80ms per-frame cost and this file could not see it, for two structural
//     reasons rather than a tuning one. It seeded no charts, so it never
//     exercised the path at all; and a frame *interval* cannot see cost that
//     fits under vsync.
//
// # Why an interval cannot see a per-frame cost
//
// The compositor paces `requestAnimationFrame` to the display period. A
// callback that finishes in 2ms and one that finishes in 10ms both hand
// control back before the next vsync, so both intervals read 16.7ms. Interval
// timing has a **blind band from 0 to about 16ms** — and that is exactly the
// band a per-frame regression lives in for its whole life before it becomes a
// dropped frame. An interval says whether frames are *late*. It cannot say
// what they *cost*, and a 16.7ms median is not evidence of health; it is the
// display telling you its own refresh rate.
//
// So there are two modes, and neither replaces the other.
//
// # `--mode=frames` — milliseconds between animation frames, while scrolling
//
// Unchanged from the version this file has always had, deliberately: its
// numbers are quoted in `PERF-D-01`, `editor.scroll-budget.spec.mjs` and
// `editor.core.js`, and they stay comparable. It answers "are frames late",
// which is the question a user actually feels. Read the **tail**, not the
// median: work that happens a few times a second leaves a 16.7ms median
// untouched, and `>20ms` / `>33ms` counts are where it shows.
//
// # `--mode=calls` — milliseconds per unit of work, and payload size
//
// Times the work itself, in a loop, with no rAF anywhere near it: build the
// payload, cross the WASM boundary, `JSON.parse` it. This is what the `CHT-13`
// worker did by hand to get `median 8.80ms` before the fix and `0.00ms` after,
// against a flat 16.7ms from the mode above. It is a first-class mode here so
// that nobody has to do it by hand again.
//
// Units are **milliseconds per call** and **characters of payload per call**
// (bytes, for the ASCII JSON these payloads are). Both are per *call*, never
// per second and never per frame — a frame may make a call zero times or twice.
//
// ## How to tell a regression from noise
//
// In this order, and the order matters:
//
//  1. **Bytes first.** A payload size is deterministic: same input, same
//     number, on any machine, with no threshold to tune. Any movement at all
//     is real. `CHT-13` was a byte problem before it was a millisecond
//     problem, and `editor.frame-budget.spec.mjs` pins it as an equality
//     between two payloads for exactly that reason.
//  2. **Then the minimum, not the median.** Noise here is one-sided: a
//     scheduler preemption, a GC, a JIT tier-up, another agent's browser on
//     the same laptop — every one of them can only make a call take *longer*.
//     Over N repetitions the minimum is the closest thing to the work's own
//     cost, and it is the statistic that moves when, and only when, the work
//     changes. A run where the median moved and the minimum did not is a run
//     that measured the machine.
//  3. **The median and p95 size the spread**, which is how much of the number
//     belongs to the machine rather than the code. A median far above the
//     minimum means the run was noisy and its median means little; a median
//     sitting on the minimum means the box was quiet.
//
// Practical thresholds on this harness: the minimum is stable to about 0.05ms
// between runs for cheap calls, so treat a shift of the **minimum** above
// ~0.2ms as real and anything smaller as needing a second run to confirm.
// Treat *any* change in payload size as real regardless of what the clock did.
//
// Two things that will otherwise be misread:
//
//   - `performance.now()` is clamped to roughly 0.1ms in Chrome. A call that
//     reports `0.00ms` is **under the clock's resolution, not free** — which
//     is what `session_chart_frames` legitimately reports today. For work that
//     cheap, the payload size is the only signal, which is the first rule
//     again.
//   - **Never compare a call-mode number with an interval-mode number.** The
//     interval is floored at the display period and the call is not. 8.80ms
//     per call and 16.7ms per frame are not in conflict; they are the two
//     halves of `PERF-13`.
//
// # `--mode=calibrate` — the profiler measuring a cost it already knows
//
// A profiler that has never been shown to detect a known cost is not a
// profiler; it is a number generator, and `PERF-13` is what a number generator
// costs. This mode injects a busy-wait of a known length into the frame and
// prints what each mode makes of it. Call mode **must** attribute it — the run
// exits non-zero if it does not, which is the check that would fail if this
// mode were ever quietly turned back into an interval measurement. Interval
// mode is **expected not to** while the burn fits under vsync, and that
// expectation is printed rather than hidden, because the contrast is the whole
// finding of `PERF-13` and it should be reproducible in one command.
//
// ## Every case gets its own browser
//
// A comparison between two numbers is only a comparison if the two runs saw
// the same sheet, and reusing one page does not give that. The editor keeps a
// draft of unsaved work in IndexedDB (`SAVE-03`), so the second load in a
// browser context comes up with the recovery bar showing — **60px of viewport,
// three rows of grid** — and the same seed measured a 30-row window in one run
// and 27 in the next, with `session_cells` payloads of 14,753 and 13,259
// characters, purely on the strength of which case had run first. Each case
// therefore runs in a fresh context, and the call report prints the window it
// measured (`[colsxrows]`) so a reader can see for himself that two lines are
// about the same sheet. The row height is pinned in the seed for the same
// reason.
//
// # Cases
//
//   --case=grid     600x40 populated cells, at 102px and 30px columns. The
//                   `PERF-D-01` sheet, kept so its numbers stay comparable.
//   --case=charts   The path `PERF-13` says this file could not exercise. Three
//                   seeds of the same sheet: no chart; a chart naming five rows
//                   of real data; and **the same chart widened to whole
//                   columns over cells that hold nothing** — which is what an
//                   `.xlsx` is free to say, is reached by opening a file rather
//                   than by having a big sheet, and cost 2,162,988 bytes and
//                   14ms a frame before `CHT-13`. `SEC-024`'s cap turned that
//                   from an out-of-memory kill into a per-frame cost; it did
//                   not remove it, and this is the case that would have shown
//                   it.
//
// # Running it
//
// With the editor served: `python3 webapp/serve.py 8123`. `OPENCALC_SMOKE_PORT`
// points it at a different one — a worktree serving its own `webapp/` cannot
// use 8123 if the main checkout is already there, and a before/after
// comparison has to run both halves against the same server.
//
// **It checks that the server is serving this checkout before it measures
// anything** (`CI-025`: `playwright.config.mjs` reuses an existing server, so
// a port that another worktree got to first silently profiles *their* bytes
// and reports them as yours). The hashes of `editor.core.js` and the WASM
// module on the wire are compared against the files on disk beside this
// script, and a mismatch aborts with exit 2 rather than printing numbers.
//
//     node frame-profile.mjs                         # both modes, all cases
//     node frame-profile.mjs --mode=calls --case=charts
//     node frame-profile.mjs --mode=calibrate --burn=8
//     node frame-profile.mjs --reps=80 --no-origin-check
import { chromium } from "@playwright/test";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const PORT = Number(process.env.OPENCALC_SMOKE_PORT ?? 8123);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// The four numbers, from a list of durations in milliseconds.
///
/// `min` is first in the report and not by accident — see the header. The
/// quantiles are nearest-rank on the sorted samples, which for these counts is
/// the honest reading; interpolating between two samples of a noisy clock
/// invents precision the clock does not have.
export function stats(samples) {
  const s = [...samples].sort((a, b) => a - b);
  const at = (q) => s[Math.min(s.length - 1, Math.floor(s.length * q))];
  return { n: s.length, min: s[0], p50: at(0.5), p95: at(0.95), max: s[s.length - 1] };
}

const ms = (n) => `${n.toFixed(2)}ms`;

/// One line of the call-mode report. Kept in one place so the reader is
/// comparing the same columns down the page.
function callLine(label, { st, bytes }) {
  const size = bytes === null ? "" : `  payload ${bytes}`;
  return `${label.padEnd(34)} n=${st.n}  min ${ms(st.min)}  p50 ${ms(st.p50)}  `
    + `p95 ${ms(st.p95)}  max ${ms(st.max)}${size}`;
}

// ---------------------------------------------------------------------------
// In-page measurement
//
// These run in the browser: Playwright ships their *source* to the page, so
// they must close over nothing from this module. Everything they need arrives
// in the single argument.
// ---------------------------------------------------------------------------

/// Time one named unit of per-frame work, repeatedly, with no rAF involved.
///
/// Returns `{ ms: [...], bytes }`. `bytes` is the payload length the last
/// repetition pulled across the boundary, or `null` for work that does not
/// cross one — it is deterministic, so one sample of it is all there is.
///
/// The probes are the calls a frame actually makes, reconstructed here rather
/// than instrumented in `editor.core.js`, because a profiler that needs the
/// subject edited to be measurable stops being run. `cells` reproduces
/// `measure()`'s viewport fetch from `state.firstRow`/`firstCol` and the window
/// `frameWindowForTest` reports; it is exact for an unfrozen sheet, and a
/// frozen one fetches from row 0 instead, so this under-reports there.
export function measureCalls({ probe, reps, warmup, burnMs }) {
  const ed = window.opencalcEditor;
  const wasm = ed.wasmApi();
  const sheet = ed.state.sheet;

  // A busy-wait, for `--mode=calibrate`. Wall-clock rather than a loop count:
  // the point is to inject a duration the *report* can be checked against, and
  // a loop count would be a different duration on every machine.
  const burn = (want) => {
    if (!want) return;
    const end = performance.now() + want;
    while (performance.now() < end) { /* deliberately spinning */ }
  };

  const probes = {
    /// The whole synchronous frame, at wherever the view is now. Everything
    /// `draw` does — geometry, the cell fetch, the chart payload, the canvas
    /// calls — but not the GPU raster that follows it.
    draw: () => { ed.draw(); return null; },
    /// The viewport's cells: build, cross, parse. What `PERF-D-01` was about.
    cells: () => {
      const w = ed.frameWindowForTest();
      const r0 = ed.state.firstRow ?? 0;
      const c0 = ed.state.firstCol ?? 0;
      const json = wasm.session_cells(
        sheet, r0, c0, r0 + Math.max(w.rowIdx - 1, 0), c0 + Math.max(w.colIdx - 1, 0),
      );
      JSON.parse(json);
      return json.length;
    },
    /// Every chart's anchor: build, cross, parse. What `CHT-13` was about, and
    /// the call this file had no case for at all until `PERF-13`.
    chartFrames: () => {
      const json = wasm.session_chart_frames(sheet);
      JSON.parse(json);
      return json.length;
    },
    /// Nothing but the burn. The calibration's own control.
    idle: () => null,
  };
  const run = probes[probe];
  if (!run) throw new Error(`unknown probe: ${probe}`);

  // Warm up outside the samples. The first call through a path pays for JIT
  // tier-up and a cold allocator, and a warm-up folded into the samples shows
  // up as a fat p95 that looks like a regression.
  for (let i = 0; i < warmup; i += 1) { burn(burnMs); run(); }

  const timings = [];
  let bytes = null;
  for (let i = 0; i < reps; i += 1) {
    const t0 = performance.now();
    burn(burnMs);
    bytes = run();
    timings.push(performance.now() - t0);
  }
  return { ms: timings, bytes };
}

/// Time the intervals between real animation frames, for `durationMs`.
///
/// `burnMs` of busy-wait per frame is injected for `--mode=calibrate`. It runs
/// in the profiler's own rAF callback rather than inside `draw`, which is the
/// same animation-frame phase and therefore the same per-frame CPU — the point
/// being to add a known cost to the frame without editing the subject.
export function measureIntervals({ durationMs, burnMs }) {
  const burn = (want) => {
    if (!want) return;
    const end = performance.now() + want;
    while (performance.now() < end) { /* deliberately spinning */ }
  };
  const t = [];
  let last = performance.now();
  const stop = performance.now() + durationMs;
  return new Promise((res) => {
    const tick = () => {
      burn(burnMs);
      const now = performance.now();
      t.push(now - last);
      last = now;
      if (now < stop) requestAnimationFrame(tick); else res(t);
    };
    requestAnimationFrame(tick);
  });
}

// ---------------------------------------------------------------------------
// Seeds
// ---------------------------------------------------------------------------

/// The `PERF-D-01` sheet: 600 rows of 40 populated columns.
///
/// **The row height is pinned, and that is not cosmetic.** The editor opens on
/// a demo workbook whose rows auto-grow to its content, and overwriting the
/// cells does not always shrink them back before the next draw — so the same
/// seed measured 30 rows of window in one run and 27 or 22 in another,
/// depending on what had run before it. Call-mode numbers are compared between
/// runs; a window that quietly changes size between them makes every such
/// comparison a comparison of two different sheets. Found by printing the
/// window beside the timing, which is why the report carries `[colsxrows]`.
export async function seedGrid(page, { rows = 600, cols = 40, rowPx = 20 } = {}) {
  await page.evaluate(([rows, cols, rowPx]) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_row_height_range(0, 0, rows - 1, rowPx);
    for (let r = 0; r < rows; r += 1) {
      for (let c = 0; c < cols; c += 1) a.session_set_cell(0, r, c, `r${r}c${c}`);
    }
  }, [rows, cols, rowPx]);
}

/// Put the sheet into one of three chart shapes, from a clean document.
///
///   `none`    — no chart; the control, so a chart's cost is a difference and
///               not an absolute nobody can calibrate.
///   `rows`    — five rows of real data across six series, with a column chart
///               over them. What a user builds by selecting a block and
///               pressing a button.
///   `columns` — that same chart, in the same place, with its series widened to
///               whole columns while the cells stay empty. What an `.xlsx` is
///               free to say. `SEC-024` caps the resolved points; `CHT-13`
///               stopped them being resolved for a frame at all.
///
/// The caller reloads the page first — `session_new()` resets the engine but
/// not the host's cached document state, and a seed that half-resets is a seed
/// whose numbers belong to the previous shape.
///
/// Returns what it actually built, so the caller can assert the seed took
/// rather than reporting a flat number for a sheet with no chart on it.
export async function seedCharts(page, shape) {
  return page.evaluate((shape) => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    if (shape !== "none") {
      // A label column and **six** numeric ones, so `session_create_chart`
      // builds six series — the shape `CHT-13` measured at 2,162,988 bytes and
      // 14ms a frame once they name whole columns. One series would be a
      // sixth of that and would understate the case by that much.
      for (let i = 0; i < 5; i += 1) {
        api.session_set_cell(0, i, 6, `R${i}`);
        for (let c = 0; c < 6; c += 1) {
          api.session_set_cell(0, i, 7 + c, String((i + 1) * 10 + c));
        }
      }
      api.session_create_chart(0, 0, 6, 4, 12, "column");
    }
    if (shape === "columns") {
      const def = JSON.parse(api.session_chart_defs(0))[0];
      const whole = (ref) => ref.replace(/\$\d+$/, () => "$1048576");
      for (const s of def.series) {
        s.values = whole(s.values);
        if (s.categories) s.categories = whole(s.categories);
      }
      api.session_set_chart(0, 0, JSON.stringify(def));
    }
    ed.draw();
    const defs = JSON.parse(api.session_chart_defs(0));
    return { charts: defs.length, refs: defs.flatMap((d) => d.series.map((s) => s.values)) };
  }, shape);
}

// ---------------------------------------------------------------------------
// `CI-025`: prove the server is serving *this* checkout
// ---------------------------------------------------------------------------

/// Compare the bytes on the wire with the bytes on disk beside this script.
///
/// `playwright.config.mjs` sets `reuseExistingServer: !process.env.CI`, so a
/// port another worktree got to first serves *their* editor and *their* WASM
/// while this script reports the numbers as yours. That has cost real time
/// twice. The WASM module matters at least as much as the JavaScript: a stale
/// `webapp/pkg` is gitignored, so nothing else in the tree will notice it.
///
/// Throws on a mismatch. The caller exits 2 — a wrong number is worse than no
/// number, and this whole file is an argument for that.
export async function assertServingThisCheckout(port) {
  const sha = (buf) => createHash("sha256").update(buf).digest("hex").slice(0, 12);
  const checks = ["editor.core.js", "pkg/casual_calc_wasm_bg.wasm"];
  const report = [];
  for (const rel of checks) {
    const local = new URL(`../../webapp/${rel}`, import.meta.url);
    let onDisk;
    try {
      onDisk = await readFile(local);
    } catch (e) {
      throw new Error(`cannot read ${local.pathname}: ${e.message}`
        + (rel.startsWith("pkg/") ? " — build it: cd crates/casual-calc-wasm && "
          + "wasm-pack build --release --target web --out-dir ../../webapp/pkg" : ""));
    }
    const res = await fetch(`http://127.0.0.1:${port}/${rel}`);
    if (!res.ok) throw new Error(`GET /${rel} -> ${res.status} from the server on ${port}`);
    const served = Buffer.from(await res.arrayBuffer());
    if (sha(served) !== sha(onDisk)) {
      throw new Error(
        `the server on ${port} is not serving this checkout: /${rel} is ${sha(served)} `
        + `on the wire and ${sha(onDisk)} on disk (${served.length} vs ${onDisk.length} bytes). `
        + `CI-025 — another worktree's server is on this port, or webapp/pkg is stale.`,
      );
    }
    report.push(`${rel} ${sha(onDisk)}`);
  }
  return report;
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

/// A page with the editor booted, in a **browser context of its own**.
///
/// Reloading the same context is not enough, and this cost an hour. The editor
/// keeps a draft of unsaved work in IndexedDB (`SAVE-03`), so the second load
/// in a context comes up with the recovery bar showing — which is **60px of
/// viewport**, three rows of grid. The same seed then measured a 30-row window
/// in one run and 27 in the next, and the difference tracked nothing but which
/// case had run before it: `session_cells` payloads of 14,753 and 13,259
/// characters for what the report called the same case.
///
/// A profiler whose numbers depend on the order its cases run in is a profiler
/// whose before-and-after comparisons are between two different sheets. A fresh
/// context starts with empty storage, so every case starts from the same
/// editor.
async function boot(browser, port) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/editor.html`);
  await page.waitForFunction(
    () => /^engine v/.test(document.querySelector("#tb-status")?.textContent || ""),
    null,
    { timeout: 30_000 },
  );
  return page;
}

/// Run `body` against a freshly booted editor, and take the context away after.
async function inFreshEditor(browser, port, body) {
  const page = await boot(browser, port);
  try {
    return await body(page);
  } finally {
    await page.context().close();
  }
}

/// Draw once and let two frames pass, so `geo` describes the sheet as it is now.
///
/// Call mode does not scroll, and an edit made through `wasmApi()` does not
/// schedule a draw — so without this the geometry `frameWindowForTest` reports
/// is whatever the *previous* case left behind. That is not a cosmetic
/// difference: the `cells` probe builds its fetch window out of it, and a
/// 30px-column case measured against a 102px window reported a *smaller*
/// payload for four times the columns. Caught by printing the window instead
/// of the timing, which is the only reason it was caught at all.
async function settle(page) {
  await page.evaluate(() => new Promise((res) => {
    window.opencalcEditor.draw();
    requestAnimationFrame(() => requestAnimationFrame(res));
  }));
}

/// Scroll the grid for `durationMs` and time the frames while it moves.
///
/// The scroll runs *during* the measurement, not before it. That is the fix
/// this file's second mistake needed and the reason it discriminates at all.
async function scrollAndTime(page, { durationMs = 1500, burnMs = 0, wheels = 200 } = {}) {
  const box = await page.locator("#grid").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const scrolling = (async () => {
    for (let i = 0; i < wheels; i += 1) await page.mouse.wheel(0, 30);
  })();
  const samples = await page.evaluate(measureIntervals, { durationMs, burnMs });
  await scrolling;
  // The first few frames are the wheel stream starting up.
  const kept = samples.slice(5);
  // The median alone cannot see work that happens a few times a second: the
  // grid is vsync-bound, so 4 expensive frames in 90 leave a 16.7ms median
  // untouched. The tail is where a periodic cost shows up, which is what the
  // accessibility mirror's staleness ceiling (`A11Y_MAX_STALE_MS`) is.
  return { ...stats(kept), over: (n) => kept.filter((x) => x > n).length };
}

/// The chart seed, with the assertions that stop a flat number being reported
/// for a sheet that has no chart on it.
async function seedCheckedCharts(page, shape) {
  const seeded = await seedCharts(page, shape);
  if (shape !== "none" && seeded.charts !== 1) {
    throw new Error(`the ${shape} seed built ${seeded.charts} charts, so it measures nothing`);
  }
  if (shape === "columns" && !seeded.refs.join().includes("1048576")) {
    throw new Error(`the columns seed did not widen: ${seeded.refs.join()}`);
  }
  return seeded;
}

/// `--mode=frames`, `--case=grid`. Unchanged in what it does or prints.
async function framesGrid(browser, port) {
  for (const w of [102, 30]) {
    await inFreshEditor(browser, port, async (page) => {
      await seedGrid(page);
      await page.evaluate(
        (w) => window.opencalcEditor.wasmApi().session_set_col_width_range(0, 0, 39, w), w,
      );
      await page.waitForTimeout(400);
      const st = await scrollAndTime(page);
      const g = await page.evaluate(() => window.opencalcEditor.frameWindowForTest());
      console.log(
        `${w}px cols: drawing ${g.cols}x${g.rows}, fetched ${g.colIdx}x${g.rowIdx} = `
        + `${g.colIdx * g.rowIdx} cells; frames ${st.n}, median ${st.p50.toFixed(1)}ms `
        + `p95 ${st.p95.toFixed(1)}ms max ${st.max.toFixed(1)}ms, `
        + `>20ms ${st.over(20)} >33ms ${st.over(33)}`,
      );
    });
  }
}

/// `--mode=frames`, `--case=charts`. The mode `PERF-13` says cannot see this,
/// run over the case it could not reach — so the blindness is reproducible and
/// not merely asserted.
async function framesCharts(browser, port) {
  for (const shape of ["none", "rows", "columns"]) {
    await inFreshEditor(browser, port, async (page) => {
      const seeded = await seedCheckedCharts(page, shape);
      await seedGrid(page, { rows: 200, cols: 12 });
      await page.waitForTimeout(300);
      const st = await scrollAndTime(page);
      const bytes = await page.evaluate(
        () => window.opencalcEditor.wasmApi().session_chart_frames(0).length,
      );
      console.log(
        `chart=${shape.padEnd(8)} charts ${seeded.charts}, payload ${String(bytes).padStart(8)}; `
        + `frames ${st.n}, median ${st.p50.toFixed(1)}ms p95 ${st.p95.toFixed(1)}ms `
        + `max ${st.max.toFixed(1)}ms, >20ms ${st.over(20)} >33ms ${st.over(33)}`,
      );
    });
  }
}

/// `--mode=calls`, `--case=grid`.
async function callsGrid(browser, { reps, warmup, port }) {
  for (const w of [102, 30]) {
    await inFreshEditor(browser, port, async (page) => {
      await seedGrid(page);
      await page.evaluate(
        (w) => window.opencalcEditor.wasmApi().session_set_col_width_range(0, 0, 39, w), w,
      );
      await settle(page);
      const g = await page.evaluate(() => window.opencalcEditor.frameWindowForTest());
      for (const probe of ["cells", "draw"]) {
        const r = await page.evaluate(measureCalls, { probe, reps, warmup, burnMs: 0 });
        console.log(callLine(
          `grid ${w}px [${g.colIdx}x${g.rowIdx}] ${probe}`, { st: stats(r.ms), bytes: r.bytes },
        ));
      }
    });
  }
}

/// `--mode=calls`, `--case=charts`. The one this row exists for.
async function callsCharts(browser, { reps, warmup, port }) {
  for (const shape of ["none", "rows", "columns"]) {
    await inFreshEditor(browser, port, async (page) => {
      await seedCheckedCharts(page, shape);
      await settle(page);
      for (const probe of ["chartFrames", "draw"]) {
        const r = await page.evaluate(measureCalls, { probe, reps, warmup, burnMs: 0 });
        console.log(callLine(`chart=${shape}  ${probe}`, { st: stats(r.ms), bytes: r.bytes }));
      }
    });
  }
}

/// `--mode=calibrate`. The profiler measuring a cost it already knows the size
/// of. Returns whether call mode attributed it; `main` turns that into an exit
/// code, because a self-check nobody notices failing is not a check.
async function calibrate(browser, { reps, warmup, burnMs, port }) {
  return inFreshEditor(browser, port, (page) => calibrateOn(page, { reps, warmup, burnMs }));
}

async function calibrateOn(page, { reps, warmup, burnMs }) {
  await seedGrid(page, { rows: 200, cols: 12 });
  await settle(page);

  const call = async (b) => stats(
    (await page.evaluate(measureCalls, { probe: "draw", reps, warmup, burnMs: b })).ms,
  );
  const without = await call(0);
  const with_ = await call(burnMs);
  const seenByCalls = with_.min - without.min;

  const frameWithout = await scrollAndTime(page, {});
  const frameWith = await scrollAndTime(page, { burnMs });
  const seenByFrames = frameWith.p50 - frameWithout.p50;

  console.log(`a ${burnMs.toFixed(1)}ms busy-wait injected into the frame:`);
  console.log(
    `  calls (draw, min)   without ${ms(without.min)}  with ${ms(with_.min)}  `
    + `delta ${seenByCalls >= 0 ? "+" : ""}${ms(seenByCalls)}`,
  );
  console.log(
    `  frames (median)     without ${ms(frameWithout.p50)}  with ${ms(frameWith.p50)}  `
    + `delta ${seenByFrames >= 0 ? "+" : ""}${ms(seenByFrames)}`,
  );

  // Half the burn, not all of it: the clock is clamped and the loop that
  // spins to `performance.now()` overshoots by up to a tick. Half is far
  // outside anything noise produces and far inside what a working timing path
  // reports, which is the band a self-check wants.
  const ok = seenByCalls >= burnMs / 2;
  console.log(
    ok
      ? `  -> call mode attributed the cost. The timing path works.`
      : `  -> CALL MODE DID NOT SEE IT (${ms(seenByCalls)} of ${ms(burnMs)}). `
        + `The timing path is broken; its numbers mean nothing.`,
  );
  if (Math.abs(seenByFrames) < burnMs / 2) {
    console.log(
      `  -> interval mode did not see it, which is expected while the burn fits `
      + `under vsync. This is PERF-13 in one run.`,
    );
  }
  return ok;
}

// ---------------------------------------------------------------------------

async function main() {
  const arg = (name, dflt) => {
    const hit = process.argv.find((a) => a.startsWith(`--${name}=`));
    return hit === undefined ? dflt : hit.slice(name.length + 3);
  };
  const mode = arg("mode", "both");
  const which = arg("case", "all");
  const reps = Number(arg("reps", 40));
  const warmup = Number(arg("warmup", 5));
  const burnMs = Number(arg("burn", 8));
  const wantGrid = which === "all" || which === "grid";
  const wantCharts = which === "all" || which === "charts";

  if (!process.argv.includes("--no-origin-check")) {
    try {
      const seen = await assertServingThisCheckout(PORT);
      console.log(`serving this checkout on ${PORT}: ${seen.join(", ")}\n`);
    } catch (e) {
      console.error(`${e.message}`);
      process.exit(2);
    }
  }

  const b = await chromium.launch();
  let ok = true;
  try {
    if (mode === "calibrate") {
      console.log("== calibrate: milliseconds, a known cost injected into the frame ==");
      ok = await calibrate(b, { reps, warmup, burnMs, port: PORT });
    } else {
      if (mode === "frames" || mode === "both") {
        console.log("== frames: milliseconds between animation frames, while scrolling ==");
        if (wantGrid) await framesGrid(b, PORT);
        if (wantCharts) await framesCharts(b, PORT);
        console.log("");
      }
      if (mode === "calls" || mode === "both") {
        console.log("== calls: milliseconds per call, and payload characters ==");
        if (wantGrid) await callsGrid(b, { reps, warmup, port: PORT });
        if (wantCharts) await callsCharts(b, { reps, warmup, port: PORT });
      }
    }
  } finally {
    await b.close();
  }
  if (!ok) process.exit(1);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) await main();
