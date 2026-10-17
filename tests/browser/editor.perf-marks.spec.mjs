// The renderer benchmark's frame-phase probes (`PERF-18`) are off for users.
//
// `draw()` carries a probe at each of its phases so `render-bench.mjs` can say
// whether a frame's cost is in fetching and laying out the data or in issuing
// canvas work. The probes cost a clock read and an object each per frame when
// on, and they are on only for a page loaded with `?perfmarks=1`. A probe left
// on by accident costs every user that on every frame and nobody would see it,
// so this pins both halves: off records nothing and leaves nothing in the
// performance timeline; on records every phase, and the phases account for the
// frame — which is what makes the benchmark's split worth reading.
//
// Counted rather than timed: whether a record exists is the same on every
// machine.

import { expect, test } from "@playwright/test";

async function seedAndScroll(page) {
  await page.evaluate(async () => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    const line = Array.from({ length: 12 }, (_, c) => (c % 3 ? "12345" : "Item text")).join("\t");
    api.session_paste_tsv(0, 0, 0, Array.from({ length: 400 }, () => line).join("\n"));
    const grid = document.querySelector("#grid-wrap");
    for (let i = 0; i < 20; i += 1) {
      grid.dispatchEvent(new WheelEvent("wheel", { deltaY: 40, bubbles: true, cancelable: true }));
      await new Promise((r) => requestAnimationFrame(r));
    }
    ed.draw();
  });
}

const boot = async (page, url) => {
  await page.goto(url);
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
};

test("without ?perfmarks=1 a frame records nothing", async ({ page }) => {
  await boot(page, "/editor.html");
  await seedAndScroll(page);
  const seen = await page.evaluate(() => ({
    frames: window.opencalcEditor.takePerfFramesForTest().length,
    measures: performance.getEntriesByName("oc:draw").length,
  }));
  expect(seen, "the frame-phase probes ran for a page that did not ask for them").toEqual({ frames: 0, measures: 0 });
});

test("with ?perfmarks=1 every draw records its phases, and they account for the frame", async ({ page }) => {
  await boot(page, "/editor.html?perfmarks=1");
  await seedAndScroll(page);
  const frames = await page.evaluate(() => window.opencalcEditor.takePerfFramesForTest());
  expect(frames.length, "no draw was recorded").toBeGreaterThan(10);
  const phases = ["measure", "fetch", "grid", "objects", "fills", "text", "borders", "merges", "overlays", "headers", "dom"];
  for (const f of frames) {
    for (const p of phases) expect(f[p], `phase ${p} missing from a frame`).toBeGreaterThanOrEqual(0);
    const parts = Object.entries(f)
      .filter(([k]) => !["t0", "total", "cells", "merges", "mergesInView"].includes(k))
      .reduce((a, [, v]) => a + v, 0);
    // The laps are contiguous, so they sum to the total exactly but for
    // floating-point rounding.
    expect(Math.abs(parts - f.total)).toBeLessThan(0.01);
  }
  // Read-and-empty: a second take sees only what was drawn since.
  expect(await page.evaluate(() => window.opencalcEditor.takePerfFramesForTest().length)).toBe(0);
});

// The merge passes in `draw()` walk only the merges that reach the drawn
// window (`PERF-18`). They walked every merge on the sheet, placing each one
// through two to four engine geometry calls before culling it, and a sheet of
// 10,000 merges took seconds a frame to paint the few in view. Counted through
// the same probe: how many merges the frame considered, against how many the
// sheet has — the same on every machine, unlike the time it saves.
test("a draw considers only the merges that reach its window", async ({ page }) => {
  await boot(page, "/editor.html?perfmarks=1");
  const seen = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    // 2,000 merges far below the first screen, one inside it, and one that
    // starts above the scrolled-to window and reaches into it.
    for (let i = 0; i < 2000; i += 1) {
      api.session_merge_cells(0, 5000 + i * 3, 0, 5001 + i * 3, 1);
      if (i % 100 === 0) api.session_clear_history();
    }
    api.session_merge_cells(0, 2, 2, 3, 4);
    ed.takePerfFramesForTest();
    ed.draw();
    const top = ed.takePerfFramesForTest().pop();
    // Scroll so that the first drawn row is inside the merge at 4998..5001.
    api.session_merge_cells(0, 4998, 3, 5001, 3);
    ed.state.scrollY = api.session_row_offset_px(0, 5000);
    ed.draw();
    const lower = ed.takePerfFramesForTest().pop();
    // What the window can show: the 2,000-run merges starting at or above its
    // last drawn row, and the one that began above it.
    const last = ed.geo.rowIdx[ed.geo.rowIdx.length - 1];
    let expected = 1;
    for (let i = 0; i < 2000; i += 1) if (5000 + i * 3 <= last) expected += 1;
    return { top, lower, first: ed.geo.rowIdx[0], expected };
  });
  expect(seen.top.merges).toBe(2001);
  expect(seen.top.mergesInView, "the frame walked merges it cannot show").toBe(1);
  // Lower down, the window holds a run of the 2,000 plus the one that began
  // above it — a merge whose anchor is off screen still paints its visible part.
  expect(seen.first).toBe(5000);
  expect(seen.expected).toBeGreaterThan(5);
  expect(seen.lower.mergesInView).toBe(seen.expected);
});
