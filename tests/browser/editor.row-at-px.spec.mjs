// `rowAtPx` is the inverse of `rowOffsetPx`, and it has to actually be one.
//
// Reported from a running editor against a real file: "while scrolling .. upper
// rows become empty and flickers", and "above cells are becoming empty on
// scrolling horizontally .. when data exceeds visual page limit .. before that
// there is no scroll so it's good".
//
// The engine stores one height per row and knows nothing about wrapped text, so
// the editor grows rows itself and folds that growth into `rowOffsetPx` /
// `rowAtPx`. The forward direction was right. The inverse iterated
// `session_row_at_px(px - growthBefore(guess))` four times and assumed it
// converged — and it does not: for a guess above the answer it subtracts *all*
// the growth, drives the pixel argument negative, the engine clamps to zero, and
// the next step lands back on the original guess. A 2-cycle, returning the
// wrong end of it.
//
// Measured on the reported file at scroll 200: it answered row 10, whose top
// edge is at 896px. At scroll 800 it answered a row past the last one, so the
// frame contained **no rows at all** and the grid painted blank — which is what
// was reported as flicker, because every scroll step lands somewhere different.
//
// Asserted as the defining property rather than against remembered numbers:
// `rowOffsetPx(r) <= px < rowOffsetPx(r + 1)`. A test that pinned "row 2 at
// 200px" would pass for any implementation that happened to hit that one case.
import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

// A sheet whose rows the editor has to grow: wrapped text in a default-width
// column is exactly the shape the reported file has.
async function seedWrapped(page) {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 40; r += 1) {
      a.session_set_cell(0, r, 0, `row ${r}`);
      a.session_set_cell(0, r, 1, "a sentence long enough to wrap several times in a default width column");
    }
    a.session_toggle_wrap(0, 0, 1, 39, 1);
    // Seeding through the engine directly bypasses the editor's own edit path,
    // so the growth map it keeps for wrapped rows is never marked stale. Say so
    // explicitly rather than relying on a redraw to notice.
    window.opencalcEditor.invalidateGrowth();
  });
  // A scroll of zero still re-measures, which is what rebuilds the growth map.
  await page.mouse.move(700, 400);
  await page.mouse.wheel(0, 1);
  await page.waitForTimeout(400);
}

test("rowAtPx is the inverse of rowOffsetPx once rows have grown", async ({ page }) => {
  const problems = await boot(page);
  await seedWrapped(page);

  const bad = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    if (!ed.growthTotal) return { skipped: "nothing grew, so this proves nothing" };
    const wrong = [];
    for (let px = 0; px <= 1600; px += 50) {
      const r = ed.rowAtPx(px);
      const top = ed.rowOffsetPx(r);
      const next = ed.rowOffsetPx(r + 1);
      if (!(top <= px && px < next)) wrong.push({ px, row: r, top, next });
    }
    return { wrong, growthTotal: ed.growthTotal };
  });

  expect(bad.skipped, "the fixture must actually grow rows").toBeUndefined();
  expect(
    bad.wrong,
    "rowAtPx returned a row whose band does not contain the pixel it was asked about",
  ).toEqual([]);
  expect(problems).toEqual([]);
});

test("scrolling a sheet with grown rows keeps rows on screen", async ({ page }) => {
  const problems = await boot(page);
  await seedWrapped(page);
  await page.mouse.move(700, 400);

  const empties = [];
  for (let i = 0; i < 12; i += 1) {
    await page.mouse.wheel(0, 150);
    const f = await page.evaluate(() => {
      const ed = window.opencalcEditor;
      return { rows: ed.frameWindowForTest().rows, items: ed.frameWindowForTest().geoItems,
               y: Math.round(ed.scrollStateForTest().scrollY) };
    });
    if (f.rows === 0) empties.push(f);
  }
  expect(
    empties,
    "the frame had no rows at all while the sheet was still scrollable — this is the blank grid",
  ).toEqual([]);
  expect(problems).toEqual([]);
});
