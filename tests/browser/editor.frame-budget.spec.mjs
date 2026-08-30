// The frame only pays for what the frame can show.
//
// PERF-D-01 measured that frame cost tracked the number of *visible columns*
// and not the size of the sheet, which ruled out a whole-sheet scan but did
// not say what the per-cell work was. Profiling the real draw path found it,
// and it was not the JSON serialisation everyone assumed: the whole viewport
// crosses the WASM boundary in ~5ms of a 26ms frame, and `JSON.parse` is under
// 1ms of that. The cost was that the frame fetched ~8x the cells it drew.
//
// `colCap`/`rowCap` are floored by `MIN_LINE = 8`, because a line *can* be 8px
// and one engine call has to be enough. On a sheet of ordinary widths that
// asked for 157 columns and 73 rows — 2920 cells — to draw 13 by 29.
//
// These are structural assertions, not wall-clock ones. A timing assertion on
// a shared CI machine is a flaky test that gets deleted; the counts are what
// was actually wrong and what a regression would change.

import { expect, test } from "@playwright/test";

async function boot(page, { cols = 40, rows = 300, width } = {}) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(([cols, rows, width]) => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    for (let r = 0; r < rows; r += 1) {
      for (let c = 0; c < cols; c += 1) a.session_set_cell(0, r, c, `r${r}c${c}`);
    }
    if (width) a.session_set_col_width_range(0, 0, cols - 1, width);
    ed.selectForTest(0, 0);
  }, [cols, rows, width]);
  await page.waitForTimeout(500);
}

const frame = (page) => page.evaluate(() => window.opencalcEditor.frameWindowForTest());

test("the lines fetched for a frame are the lines the frame can show", async ({ page }) => {
  await boot(page, { width: 102 });
  const w = await frame(page);
  // One line past the edge is kept on purpose: it is where a spilling label
  // clips, and what colAtX/rowAtY fall back to.
  expect(w.cols, "the window is empty, so this proves nothing").toBeGreaterThan(4);
  expect(w.colIdx, `${w.colIdx} columns fetched to draw ${w.cols}`).toBeLessThanOrEqual(w.cols + 1);
  expect(w.rowIdx, `${w.rowIdx} rows fetched to draw ${w.rows}`).toBeLessThanOrEqual(w.rows + 1);
});

test("narrow columns do not change the ratio", async ({ page }) => {
  // The original measurement saw cost double from 12 to 40 visible columns.
  // With narrow columns more of the over-fetch was real, which is why the
  // symptom tracked column count rather than sheet size.
  await boot(page, { width: 30 });
  const w = await frame(page);
  expect(w.cols).toBeGreaterThan(20);
  expect(w.colIdx, `${w.colIdx} columns fetched to draw ${w.cols}`).toBeLessThanOrEqual(w.cols + 1);
});

test("a frame holds at most one spill owner per row per side", async ({ page }) => {
  await boot(page, { cols: 3, rows: 60, width: 90 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    // A wide band of populated columns to the left of the window. The band
    // between the furthest owner and the window is not empty — only the
    // columns *outside the span* are — so the old gather pushed every
    // populated cell in it, which on a real sheet is unbounded.
    for (let r = 0; r < 40; r += 1) {
      a.session_set_cell(0, r, 0, `a very long label in row ${r} that spills a long way right`);
      for (let c = 1; c < 40; c += 1) a.session_set_cell(0, r, c, `x${r}c${c} is itself long enough to spill`);
    }
    window.opencalcEditor.selectForTest(0, 90);
  });
  await page.waitForTimeout(400);
  const w = await frame(page);
  // At most one owner per drawn row per side.
  expect(w.geoItems, `${w.geoItems} cells held for a ${w.colIdx}x${w.rowIdx} window`)
    .toBeLessThanOrEqual(w.colIdx * w.rowIdx + 2 * w.rowIdx);
});

test("a label blocked by a nearer cell is not held as a spill owner", async ({ page }) => {
  await boot(page, { cols: 1, rows: 8, width: 80 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    // The blocker has to exist in *every* seeded row. With it only in row 0,
    // rows 1-7 have nothing between their own text and the window, so holding
    // column 0 for them is correct — and the test would fail for a reason that
    // is not the defect.
    for (let r = 0; r < 8; r += 1) {
      a.session_set_cell(0, r, 0, `a very long label in row ${r} that would spill a long way right`);
      a.session_set_cell(0, r, 8, "12345"); // a number: blocks, and cannot spill itself
    }
    // Far enough that column 8 is *outside* the window too. A blocker inside
    // the window is handled by the draw scan, not the gather, so leaving it
    // visible tests a different mechanism than the one this is about.
    window.opencalcEditor.selectForTest(0, 60);
  });
  await page.waitForTimeout(400);
  const w = await frame(page);
  // Column 0 is behind the blocker, so it must not reach the window. Filtering
  // to spillable text *before* reducing let the number fall out of the set and
  // the far label was drawn into a window Excel would not show it in.
  expect(w.spillCols, `column 0 was held although column 8 blocks it`).not.toContain(0);
});

test("scrolling back over drawn rows does not measure their text again", async ({ page }) => {
  await boot(page, { cols: 12, rows: 200, width: 90 });
  const box = await page.locator("#grid").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 30; i += 1) await page.mouse.wheel(0, 60);
  await page.waitForTimeout(400);
  await page.evaluate(() => window.opencalcEditor.resetTextWidthStatsForTest());
  for (let i = 0; i < 30; i += 1) await page.mouse.wheel(0, -60);
  await page.waitForTimeout(400);
  const s = await page.evaluate(() => window.opencalcEditor.textWidthStatsForTest());
  // These rows were measured on the way down. Re-measuring them on the way
  // back up was 17% of frame time.
  expect(s.misses, `${s.misses} re-measurements over rows already drawn (${s.hits} cached)`)
    .toBeLessThan(s.hits / 4 + 20);
});

test("a chart costs the frame its anchor, not the range its series name", async ({ page }) => {
  // `CHT-13`. The frame asked the engine for every chart's series with every
  // point *resolved* and used eight integers of the answer — the anchor cells
  // and their EMU offsets — because the picture itself has come from
  // `session_chart_items` since `RND-10`.
  //
  // The cost was not bounded by the data on the sheet. A series reference is a
  // string out of an untrusted `.xlsx`, so `$H$1:$H$1048576` cost
  // `MAX_SERIES_POINTS` nulls per series per frame with nothing in those
  // cells: 2 MB a frame against a 16.7 ms budget, on an empty sheet.
  //
  // Stated as an equality rather than a ceiling. A byte budget would need
  // re-tuning whenever a field is added; that the number does not *move* is
  // the invariant, and it is what a regression would break.
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  const seen = await page.evaluate(async () => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    const settle = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

    for (let i = 0; i < 5; i += 1) {
      api.session_set_cell(0, i, 6, `R${i}`);
      api.session_set_cell(0, i, 7, String((i + 1) * 10));
    }
    api.session_create_chart(0, 0, 6, 4, 7, "column");
    ed.draw();
    await settle();
    const small = ed.chartPayloadBytesForTest();

    // The same chart in the same place, with its references widened to whole
    // columns — which is what a file is free to say, and what the cells hold
    // nothing extra for.
    const def = JSON.parse(api.session_chart_defs(0))[0];
    const wholeColumn = (ref) => ref.replace(/\$\d+$/, () => "$1048576");
    for (const s of def.series) {
      s.values = wholeColumn(s.values);
      if (s.categories) s.categories = wholeColumn(s.categories);
    }
    api.session_set_chart(0, 0, JSON.stringify(def));
    ed.draw();
    await settle();

    return {
      small,
      whole: ed.chartPayloadBytesForTest(),
      charts: ed.chartFrames.length,
      widened: JSON.parse(api.session_chart_defs(0))[0].series.map((s) => s.values),
    };
  });

  expect(seen.charts, "the chart survived the edit, so this measures something").toBe(1);
  expect(seen.small, "the payload is not empty, so equality proves something").toBeGreaterThan(0);
  expect(seen.widened.join(), "the references really were widened").toMatch(/1048576/);
  expect(
    seen.whole,
    `${seen.whole} bytes crossed for a chart naming whole columns against ${seen.small} for the same chart naming five rows`,
  ).toBe(seen.small);
});
