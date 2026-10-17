// What a scroll frame costs on a large sheet (`PERF-22`, `PERF-23`, `PERF-24`).
//
// `XL-UX-183` measured three per-frame costs that grew with the *sheet* rather
// than with the window, each about 200 ms a frame at 100k cells:
//
//   - `PERF-22` — `usedBounds()` asked the engine for the used range on every
//     frame (the scrollbars and the grid's ARIA counts both want it), and the
//     engine answers by walking every stored cell.
//   - `PERF-23` — with frozen panes `measure()` fetched from row and column 0,
//     so at row 20,000 a frame pulled every cell scrolled past.
//   - `PERF-24` — every `session_cells` call recomputed each conditional
//     format's range statistics over its whole applies-to range.
//
// So this has two kinds of assertion. The **structural** ones — no used-range
// scan, no statistics pass, no more cells held than the window shows — are what
// was wrong and what a regression would change, and they do not care how fast
// the machine is. The **wall-clock** one is the target itself (docs/30: 60 fps,
// so 16.7 ms a frame), scaled for a slower machine the way
// `editor.open-save-worker.spec.mjs` scales its limit: by timing a fixed piece
// of engine work this change does not touch, on this machine, first.
//
// Each cache gets a staleness test beside it: an edit by the editor, by a
// `session_*` call, by undo, and a new document must all show on the next
// frame. A cache that can go stale is worse than no cache.
import { expect, test } from "@playwright/test";

const ROWS = 20_500;
const COLS = 5;
/// The row the frames are measured at — far enough that a fetch from row 0
/// is 100k cells.
const DEEP = 20_000;
/// docs/30: 60 fps.
const FRAME_BUDGET_MS = 16.7;
/// What seeding this sheet (one `session_paste_tsv` of 102,500 cells) took on
/// the machine the budget was set on. The ratio of this machine's time to it
/// scales the budget, never below 1.
const REFERENCE_SEED_MS = 75;

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

/// 100k cells, three conditional formats over the whole data (a colour scale,
/// a data bar and a top-N — all three need range statistics), and merges both
/// near the top and at the depth the frames are measured at.
async function seed(page) {
  return await page.evaluate(([ROWS, COLS]) => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    c.switchSheet(a.session_add_sheet());
    const s = c.state.sheet;
    const lines = [];
    for (let r = 0; r < ROWS; r += 1) {
      lines.push(`${r}\t${(r * 7) % 1013}\t${(r * 13) % 997}\tname ${r}\t${r % 17}`);
    }
    const tsv = lines.join("\n");
    const t = performance.now();
    a.session_paste_tsv(s, 0, 0, tsv);
    const seedMs = performance.now() - t;
    a.session_add_cf(s, 0, 0, ROWS - 1, 0, "colorscale", 0, 0, "F8696B,FFEB84,63BE7B", "");
    a.session_add_cf(s, 0, 1, ROWS - 1, 1, "databar", 0, 0, "638EC6", "");
    a.session_add_cf(s, 0, 2, ROWS - 1, 2, "top", 10, 0, "", "FFC7CE");
    for (const r of [2, 40, 19_990, 20_010, 20_030]) {
      a.session_set_cell(s, r, 6, `merged ${r}`);
      a.session_merge_cells(s, r, 6, r + 2, 8);
    }
    c.selectForTest(0, 0);
    c.draw();
    return { seedMs, cells: ROWS * COLS };
  }), [ROWS, COLS]);
}

/// Draw `frames` frames scrolling down a row at a time from row `DEEP`,
/// synchronously and timed — the cost of the frame, not the interval between
/// animation frames, which cannot see anything under vsync (see
/// `frame-profile.mjs`).
async function scrollFrames(page, { freeze, frames = 24 }) {
  return await page.evaluate(([freeze, frames, DEEP]) => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    const s = c.state.sheet;
    a.session_set_freeze(s, freeze, freeze);
    const frozenH = freeze ? a.session_row_offset_px(s, freeze) : 0;
    c.state.scrollX = 0;
    c.state.scrollY = a.session_row_offset_px(s, DEEP) - frozenH;
    c.draw(); // warm: the first frame after a freeze change rebuilds caches
    c.draw();
    const before = {
      bounds: c.usedBoundsStatsForTest?.().scans ?? null,
      stats: typeof a.session_cf_stats_computed === "function" ? a.session_cf_stats_computed() : null,
    };
    const times = [];
    let held = 0;
    for (let i = 0; i < frames; i += 1) {
      c.state.scrollY += 20;
      const t = performance.now();
      c.draw();
      times.push(performance.now() - t);
      held = Math.max(held, c.frameWindowForTest().geoItems);
    }
    const w = c.frameWindowForTest();
    times.sort((x, y) => x - y);
    return {
      median: times[times.length >> 1],
      max: times[times.length - 1],
      held,
      window: w.colIdx * w.rowIdx,
      rows: w.rowIdx,
      boundsScans: before.bounds === null ? null : c.usedBoundsStatsForTest().scans - before.bounds,
      statsRuns: before.stats === null ? null : a.session_cf_stats_computed() - before.stats,
      firstRow: c.state.firstRow,
    };
  }), [freeze, frames, DEEP]);
}

const round = (m) => Object.fromEntries(Object.entries(m).map(([k, v]) => [k, typeof v === "number" ? Math.round(v * 10) / 10 : v]));

test("a scroll frame at row 20,000 of a 100k-cell sheet fits the frame budget, frozen or not", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  const { seedMs } = await seed(page);
  const budget = FRAME_BUDGET_MS * Math.max(1, seedMs / REFERENCE_SEED_MS);
  const open = await scrollFrames(page, { freeze: 0 });
  const frozen = await scrollFrames(page, { freeze: 1 });
  console.log(
    `seed ${Math.round(seedMs)} ms (reference ${REFERENCE_SEED_MS}); budget ${budget.toFixed(1)} ms; ` +
    `unfrozen ${JSON.stringify(round(open))}; frozen 1x1 ${JSON.stringify(round(frozen))}`,
  );
  expect(open.firstRow, "the frames were drawn where they were meant to be").toBeGreaterThanOrEqual(20_000);
  expect(frozen.firstRow).toBeGreaterThanOrEqual(20_000);
  expect(open.median, `unfrozen frame ${open.median.toFixed(1)} ms against ${budget.toFixed(1)} ms`).toBeLessThan(budget);
  expect(frozen.median, `frozen frame ${frozen.median.toFixed(1)} ms against ${budget.toFixed(1)} ms`).toBeLessThan(budget);
  // PERF-23's acceptance: freezing a row and a column costs the frame next to
  // nothing — within 10%, plus a millisecond for timer noise at this scale.
  expect(
    frozen.median,
    `frozen ${frozen.median.toFixed(1)} ms against unfrozen ${open.median.toFixed(1)} ms`,
  ).toBeLessThanOrEqual(open.median * 1.1 + 1);
});

test("a scroll frame does not scan the used range (PERF-22)", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  await seed(page);
  const m = await scrollFrames(page, { freeze: 0, frames: 12 });
  expect(m.boundsScans, "the editor exposes its used-range scan count").not.toBeNull();
  expect(m.boundsScans, `${m.boundsScans} used-range scans over 12 frames with nothing edited`).toBe(0);
});

test("a scroll frame does no conditional-format statistics work (PERF-24)", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  await seed(page);
  const m = await scrollFrames(page, { freeze: 0, frames: 12 });
  expect(m.statsRuns, "the engine exposes its statistics count").not.toBeNull();
  expect(m.statsRuns, `${m.statsRuns} range-statistics passes over 12 frames with nothing edited`).toBe(0);
});

test("with a frozen row and column a frame holds the window, not every cell scrolled past (PERF-23)", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  await seed(page);
  const m = await scrollFrames(page, { freeze: 1, frames: 6 });
  // The window, plus an edge line each way for borders, plus one spill owner
  // per row per side.
  expect(m.held, `${m.held} cells held for a ${m.window}-cell window at row ${m.firstRow}`)
    .toBeLessThanOrEqual(m.window + 4 * m.rows + 64);
});

// --- Staleness: every cache must follow every kind of edit -----------------

test("the used range follows an edit by the editor, by the engine, by undo and by a new document (PERF-22)", async ({ page }) => {
  await boot(page);
  const counts = await page.evaluate(() => import("/editor.core.js").then(async (c) => {
    const a = c.wasm;
    const grid = document.querySelector("#grid");
    const rows = () => { c.draw(); return Number(grid.getAttribute("aria-rowcount")); };
    c.switchSheet(a.session_add_sheet());
    const s = c.state.sheet;
    const out = { empty: rows() };
    // The engine directly, as a macro or an agent would.
    a.session_set_cell(s, 499, 0, "far");
    out.engine = rows();
    // Undo.
    a.session_undo();
    out.undone = rows();
    // The editor's own edit path: type into a cell and commit.
    c.selectForTest(899, 0);
    c.draw();
    await new Promise((r) => setTimeout(r, 50));
    return out;
  }));
  // Typing goes through the real keyboard path.
  await page.keyboard.type("typed");
  await page.keyboard.press("Enter");
  const typed = await page.evaluate(() => import("/editor.core.js").then((c) => {
    c.draw();
    return Number(document.querySelector("#grid").getAttribute("aria-rowcount"));
  }));
  const fresh = await page.evaluate(() => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    const rows = () => { c.draw(); return Number(document.querySelector("#grid").getAttribute("aria-rowcount")); };
    // Two new documents with the same sheet and the same edit count — one
    // edit each — and different used ranges: a cache keyed on the edit
    // counter hands the second the first one's answer.
    a.session_new();
    c.switchSheet(0);
    const empty = rows();
    a.session_set_cell(0, 99, 0, "x");
    const first = rows();
    a.session_new();
    c.switchSheet(0);
    a.session_set_cell(0, 299, 0, "x");
    return { empty, first, edited: rows() };
  }));
  // aria-rowcount is max(used rows + 30, last drawn row) + 1.
  expect(counts.engine, "a session_set_cell at row 500 shows on the next frame").toBe(500 + 30 + 1);
  expect(counts.undone, "undoing it shows on the next frame").toBe(counts.empty);
  expect(typed, "a cell typed at row 900 shows on the next frame").toBe(900 + 30 + 1);
  expect(fresh.edited, "a new document's edit shows on the next frame").toBe(300 + 30 + 1);
  expect(fresh.first, "the first new document's edit shows").toBe(100 + 30 + 1);
  expect(fresh.empty).toBeLessThan(100);
});

test("conditional-format statistics follow an edit, an undo, a recalculation and a new document (PERF-24)", async ({ page }) => {
  await boot(page);
  const seen = await page.evaluate(() => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    // Sheet 0 of a new book, so the new book at the end asks about the same
    // sheet index — the case a cache keyed on the sheet alone gets wrong.
    a.session_new();
    const s = 0;
    for (let r = 0; r < 10; r += 1) a.session_set_cell(s, r, 0, String(r));
    a.session_add_cf(s, 0, 0, 19, 0, "colorscale", 0, 0, "FF0000,00FF00", "");
    // A5 holds 4 of 0..9: somewhere in the middle of the scale.
    const fill = () => JSON.parse(a.session_cells(s, 4, 0, 4, 0)).find((t) => t.r === 4 && t.c === 0)?.bg;
    const out = { start: fill() };
    out.again = fill(); // cached: the same answer
    // A new maximum moves where 4 sits on the scale.
    a.session_set_cell(s, 10, 0, "1000");
    out.widened = fill();
    a.session_undo();
    out.undone = fill();
    // A recalculation that changes a value with no edit to that value — F9
    // in manual mode, which does not move the edit counter at all.
    a.session_set_cell(s, 11, 0, "=A2*1000"); // 1000: the scale's maximum
    a.session_set_calculation_mode("manual");
    a.session_set_cell(s, 1, 0, "0"); // A12 is now stale, still 1000
    out.beforeRecalc = fill();
    a.session_recalculate(); // A12 becomes 0, and 9 is the maximum again
    out.afterRecalc = fill();
    a.session_set_calculation_mode("auto");
    // A new document: the same sheet index, a different book.
    a.session_new();
    for (let r = 0; r < 10; r += 1) a.session_set_cell(0, r, 0, String(9 - r));
    a.session_add_cf(0, 0, 0, 9, 0, "colorscale", 0, 0, "FF0000,00FF00", "");
    out.fresh = JSON.parse(a.session_cells(0, 4, 0, 4, 0)).find((t) => t.r === 4)?.bg;
    return out;
  }));
  expect(seen.start, "the rule paints the cell").toBeTruthy();
  expect(seen.again).toBe(seen.start);
  expect(seen.widened, "a new maximum recolours the cell").not.toBe(seen.start);
  expect(seen.undone, "undo restores the colour").toBe(seen.start);
  expect(seen.afterRecalc, "a recalculation that changes the maximum recolours the cell").not.toBe(seen.beforeRecalc);
  expect(seen.fresh, "a new document's statistics, not the last one's").toBeTruthy();
  expect(seen.fresh, "a new document's statistics, not the last one's").not.toBe(seen.undone);
});
