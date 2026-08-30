// The selection has to be visible on cells that paint their own background
// (`UX-SEL-06`).
//
// Reported from a running editor: selecting a whole column inside a table
// highlighted the empty cells below it and **none of the table's own rows**, so
// the only surviving mark was the active cell's outline — "we can only see the
// blue box created on the first row of that column".
//
// The cause was paint order. The tint was drawn early, and everything opaque
// drawn afterwards erased it: table shading, banded rows, conditional
// formatting, any cell with a fill. It is drawn after those now and before the
// text, which is what the translucent `--oc-selection-color` was always for.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1100, height: 800 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.waitForTimeout(500);
}

/// The canvas pixel at the centre of a cell, as `r,g,b`.
const pixelAt = (page, row, col) => page.evaluate(([r, c]) => {
  const ed = window.opencalcEditor;
  const box = ed.cellBoxForTest ? ed.cellBoxForTest(r, c) : null;
  const canvas = document.querySelector("canvas");
  const dpr = window.devicePixelRatio || 1;
  const ctx = canvas.getContext("2d");
  const x = Math.round((box.x + box.w / 2) * dpr);
  const y = Math.round((box.y + box.h / 2) * dpr);
  const d = ctx.getImageData(x, y, 1, 1).data;
  return `${d[0]},${d[1]},${d[2]}`;
}, [row, col]);

test("selecting a column tints the cells that have a fill of their own", async ({ page }) => {
  await boot(page);

  // A cell with an explicit background — the same situation a table's banding,
  // or a conditional format, puts every cell of a column into.
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (let r = 0; r < 6; r += 1) a.session_set_cell(0, r, 1, String(r + 1));
    a.session_set_fill(0, 1, 1, 4, 1, "FFE699");
  });
  // **A repaint has to happen before the canvas is read.** `session_set_fill`
  // writes the model and schedules nothing, so the first version of this
  // sampled a frame drawn *before* the fill existed — it compared a stale white
  // pixel against a painted one, and passed whichever way the paint order went.
  // Moving the selection is a route a user has and forces the frame.
  await page.fill("#cell-ref", "A1");
  await page.press("#cell-ref", "Enter");
  await page.waitForTimeout(500);

  const unselected = await pixelAt(page, 2, 1);
  // The fill itself, so a later failure cannot be a fill that never landed.
  expect(unselected, "the cell has no fill, so this proves nothing about fills").toBe("255,230,153");

  // The whole column, through the Name Box — a route a user has, rather than a
  // helper written for this test. B3 is then inside the selection and is not
  // the anchor, so what it shows is the tint and not the active cell's box.
  await page.fill("#cell-ref", "B:B");
  await page.press("#cell-ref", "Enter");
  await page.waitForTimeout(400);
  const selected = await pixelAt(page, 2, 1);

  expect(selected,
    `a filled cell looks identical selected and unselected (${unselected}), so the selection is invisible on it`)
    .not.toBe(unselected);

  // And an empty cell in the same column is still tinted, unchanged by the fix.
  const empty = await pixelAt(page, 8, 1);
  expect(empty, "the empty cells of the column lost their tint").not.toBe("255,255,255");
});
