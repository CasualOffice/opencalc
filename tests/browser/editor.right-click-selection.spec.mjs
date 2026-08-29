// A right-click does not throw away the selection it was aimed at.
//
// Reported from a running editor: Ctrl+A took the block, a right-click inside
// it left one cell selected, and every verb in the menu then acted on that cell
// rather than on what the user was looking at — Delete included.
//
// The cause was ordering, not policy. `mousedown` fires *before* `contextmenu`,
// so the right button reached the ordinary select path and collapsed the
// selection before the menu opened. The `contextmenu` handler already had the
// right rule — keep a selection you clicked inside, move one you clicked
// outside, which is what Excel and Sheets both do — and never got the chance to
// apply it.
//
// Both halves are asserted here. Fixing the first by making the right button
// inert everywhere would have broken the second, and the second is the commoner
// gesture: right-clicking a cell you have not selected yet.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    const hdr = ["Region", "Rep", "Units", "Revenue"];
    hdr.forEach((h, c) => a.session_set_cell(0, 0, c, h));
    for (let r = 1; r < 7; r += 1) {
      hdr.forEach((_, c) => a.session_set_cell(0, r, c, c < 2 ? `v${r}${c}` : String(r * 10 + c)));
    }
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.waitForTimeout(250);
}

const centre = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const e = window.opencalcEditor;
    return { x: e.colXAt(c) + e.colWAt(c) / 2, y: e.rowYAt(r) + e.rowHAt(r) / 2 };
  }, [r, c]);
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());

test("right-clicking inside a select-all keeps the whole selection", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+a");
  await page.waitForTimeout(200);
  const all = await sel(page);
  expect(all.r1 - all.r0, "Ctrl+A took more than one row").toBeGreaterThan(0);

  const p = await centre(page, 3, 2);
  await page.mouse.click(box.x + p.x, box.y + p.y, { button: "right" });
  await page.waitForTimeout(250);

  expect(await sel(page), "the selection survived the right-click").toEqual(all);
  await expect(page.locator("#sheet-ctx")).toBeVisible();
});

test("right-clicking inside a dragged range keeps it", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const a = await centre(page, 1, 0), b = await centre(page, 4, 2);
  await page.mouse.move(box.x + a.x, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + b.x, box.y + b.y, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(200);
  const range = await sel(page);
  expect(range.c1 - range.c0, "a range was dragged").toBeGreaterThan(0);

  const inside = await centre(page, 2, 1);
  await page.mouse.click(box.x + inside.x, box.y + inside.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await sel(page), "the dragged range survived").toEqual(range);
});

test("right-clicking outside a selection still moves it", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const near = await centre(page, 1, 1);
  await page.mouse.click(box.x + near.x, box.y + near.y);
  await page.waitForTimeout(150);

  const far = await centre(page, 5, 3);
  await page.mouse.click(box.x + far.x, box.y + far.y, { button: "right" });
  await page.waitForTimeout(250);
  // The commoner gesture, and the one a blanket "ignore the right button" fix
  // would have broken: right-clicking a cell you have not selected yet.
  expect(await sel(page)).toEqual({ r0: 5, c0: 3, r1: 5, c1: 3 });
});

// The four selection kinds reach four different branches of the right-click
// handler, and only two of them were exercised by the report. `SEC-022` is what
// this class costs when it is not caught: *"Remove Duplicates destroyed data
// the user never selected"*. The handler's own header branches already carry a
// comment about it — "every verb in it then acted on the wrong target,
// including Delete" — so the rule was known and only partly enforced.
test("a right-click preserves every kind of selection it lands inside", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await page.evaluate(() => {
    const s = window.opencalcEditor.scrollStateForTest();
    return { hw: s.bodyX0, hh: s.bodyY0 };
  });

  // A column band: click its header, then right-click a cell inside it.
  const colX = (await centre(page, 0, 1)).x;
  await page.mouse.click(box.x + colX, box.y + g.hh / 2);
  await page.waitForTimeout(150);
  const cols = await sel(page);
  const inCol = await centre(page, 3, 1);
  await page.mouse.click(box.x + inCol.x, box.y + inCol.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await sel(page), "a column selection survived").toEqual(cols);
  await page.keyboard.press("Escape");

  // A row band, the same way.
  const rowY = (await centre(page, 2, 0)).y;
  await page.mouse.click(box.x + g.hw / 2, box.y + rowY);
  await page.waitForTimeout(150);
  const rows = await sel(page);
  const inRow = await centre(page, 2, 2);
  await page.mouse.click(box.x + inRow.x, box.y + inRow.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await sel(page), "a row selection survived").toEqual(rows);
});
