// A row's height is the row's, not the viewport's.
//
// Reported by the owner with a screen recording of a real file (2026-10-17):
// scrolling right, "as soon as one column passed from rendering" every row
// changed height, and changed back when it returned. The file's address column
// held text with a line break but no Wrap Text. `measure()` re-grew rows each
// frame from the cells *on screen*, and counted that line break as two lines;
// the document-wide growth map, which the scroll offsets use, did not count it
// at all. So a row's drawn height depended on which columns were in view, and
// disagreed with the offsets everything else was positioned by.
//
// Excel: a row's height does not depend on scrolling, and a line break shows as
// a break only when Wrap Text is on — without it the text stays on one line and
// the row keeps its height ("Wrap text in a cell": a line break within a cell
// is made with Alt+Enter, which turns Wrap Text on).
import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  // A sheet of its own: the demo's rows have their own fonts and heights.
  await page.evaluate(() => import("/editor.core.js").then((c) => c.switchSheet(c.wasm.session_add_sheet())));
}

/// Heights of the drawn rows, by row index.
const drawnHeights = (page) => page.evaluate(() => import("/editor.core.js").then((c) => {
  const out = {};
  c.geo.rowIdx.forEach((r, i) => { out[r] = c.geo.rowH[i]; });
  return out;
}));

async function seed(page, { wrapLineBreaks = false } = {}) {
  await page.evaluate((wrapLineBreaks) => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    const s = c.state.sheet;
    for (let r = 0; r < 30; r += 1) {
      a.session_set_cell(s, r, 0, `row ${r + 1}`);
      // Column B: wrapped prose, so its rows must be tall wherever B is.
      if (r % 3 === 0) a.session_set_cell(s, r, 1, "a sentence long enough to wrap several times in a default width column");
      // Column D: a line break and no Wrap Text — the reported file's shape.
      a.session_set_cell(s, r, 3, `${r} Some Street\nSomewhere, ST 0000${r}`);
    }
    // Content far to the right, so there is somewhere to scroll to.
    a.session_set_cell(s, 0, 60, "far right");
    a.session_toggle_wrap(s, 0, 1, 29, 1);
    if (wrapLineBreaks) a.session_toggle_wrap(s, 0, 3, 29, 3);
    c.draw();
  }), wrapLineBreaks);
  await page.waitForTimeout(200);
}

test("scrolling right never changes a row's height", async ({ page }) => {
  await boot(page);
  await seed(page);
  const grid = page.locator("#grid");
  const box = await grid.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);

  const first = await drawnHeights(page);
  const changed = [];
  for (let step = 1; step <= 30; step += 1) {
    await page.mouse.wheel(45, 0);
    await page.waitForTimeout(30);
    const now = await drawnHeights(page);
    for (const [r, h] of Object.entries(now)) {
      if (first[r] !== undefined && first[r] !== h) changed.push(`step ${step}: row ${+r + 1} ${first[r]}→${h}`);
    }
  }
  const left = await page.evaluate(() => import("/editor.core.js").then((c) => c.state.firstCol));
  expect(left, "the scroll must have taken columns B and D out of view").toBeGreaterThan(3);
  expect(changed, "a row's height moved while scrolling").toEqual([]);
});

test("a line break without Wrap Text keeps the row one line high, as in Excel", async ({ page }) => {
  await boot(page);
  await seed(page);
  const h = await drawnHeights(page);
  const base = await page.evaluate(() => import("/editor.core.js").then((c) => JSON.parse(c.wasm.session_autofit_candidates(c.state.sheet)).default));
  // Row 2 has the line-break address but no wrapped prose.
  expect(h[1]).toBe(base);
  // Row 1 has wrapped prose in B: it grows.
  expect(h[0]).toBeGreaterThan(base);
});

test("with Wrap Text on, the line break makes the row two lines, wherever the scroll is", async ({ page }) => {
  await boot(page);
  await seed(page, { wrapLineBreaks: true });
  const before = await drawnHeights(page);
  const base = await page.evaluate(() => import("/editor.core.js").then((c) => JSON.parse(c.wasm.session_autofit_candidates(c.state.sheet)).default));
  expect(before[1]).toBeGreaterThan(base);
  const box = await page.locator("#grid").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 20; i += 1) { await page.mouse.wheel(45, 0); await page.waitForTimeout(20); }
  const after = await drawnHeights(page);
  expect(after[1]).toBe(before[1]);
});

test("an edit that reaches the engine by any path re-sizes its row", async ({ page }) => {
  await boot(page);
  await seed(page);
  const base = await page.evaluate(() => import("/editor.core.js").then((c) => JSON.parse(c.wasm.session_autofit_candidates(c.state.sheet)).default));
  // Straight to the engine, with no editor bookkeeping: a remote edit, a macro
  // or an agent arrives this way, and nothing tells the growth map to rebuild.
  await page.evaluate(() => import("/editor.core.js").then((c) => {
    c.wasm.session_toggle_wrap(c.state.sheet, 1, 3, 1, 3);
    c.draw();
  }));
  const h = await drawnHeights(page);
  expect(h[1]).toBeGreaterThan(base);
});

test("AutoFit Row Height sizes a line break without Wrap Text as one line, as Excel does", async ({ page }) => {
  await boot(page);
  await seed(page);
  const base = await page.evaluate(() => import("/editor.core.js").then((c) => JSON.parse(c.wasm.session_autofit_candidates(c.state.sheet)).default));
  // Row 2: the line-break address in D, nothing wrapped. AutoFit persists the
  // height it measures, so a two-line answer here would pin the row tall.
  await page.evaluate(() => import("/editor.selection.js").then((s) => s.autofitRowForTest(1)));
  const h = await drawnHeights(page);
  expect(h[1]).toBe(base);
});
