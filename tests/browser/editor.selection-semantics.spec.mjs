// Excel's selection semantics: the active cell, the name box, and Enter
// inside a block (fit-gap XL-UX-011..014).

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const nameBox = (page) => page.locator("#cell-ref");
const box = (page, r, c) => page.evaluate(([r, c]) => {
  const b = window.opencalcEditor.cellBoxForTest(r, c);
  const g = document.getElementById("grid").getBoundingClientRect();
  return { x: g.left + b.x + b.w / 2, y: g.top + b.y + b.h / 2 };
}, [r, c]);

/// XL-UX-014: rows×columns while the selection is being made, then the active
/// cell — "2R x 2C" stayed in the name box after Shift was let go.
test("the name box returns to the active cell when extending ends", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));   // B2
  await page.keyboard.down("Shift");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await expect(nameBox(page), "while extending").toHaveValue("2R x 2C");
  await page.keyboard.up("Shift");
  await expect(nameBox(page), "after Shift is released").toHaveValue("B2");
});

/// XL-UX-011: Shift+click extends and the active cell stays at the anchor.
/// (The audit's probe passed `modifiers` to `mouse.click`, which Playwright
/// ignores — Shift was never held. Held for real, this is Excel's behaviour.)
test("Shift+click extends and keeps the active cell at the anchor", async ({ page }) => {
  await boot(page);
  const a = await box(page, 1, 1), c = await box(page, 2, 2);
  await page.mouse.click(a.x, a.y);
  await page.keyboard.down("Shift");
  await page.mouse.click(c.x, c.y);
  await page.keyboard.up("Shift");
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 1, c0: 1, r1: 2, c1: 2 });
  await expect(nameBox(page)).toHaveValue("B2");
});

/// XL-UX-012: Enter walks inside the selected block and wraps.
test("Enter cycles inside a selected block", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.down("Shift");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.up("Shift");
  const seen = [];
  for (let i = 0; i < 4; i++) { await page.keyboard.press("Enter"); seen.push(await nameBox(page).inputValue()); }
  expect(seen).toEqual(["B3", "C2", "C3", "B2"]);
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 1, c0: 1, r1: 2, c1: 2 });
});

/// XL-UX-013: Ctrl+A selects the current region without moving the active cell.
test("Ctrl+A keeps the active cell where it was", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.press("ControlOrMeta+a");
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 0, c0: 0, r1: 4, c1: 3 });
  await expect(nameBox(page)).toHaveValue("B2");
});

/// Shift+arrow after Ctrl+A resizes the region from its far edge. With the
/// active cell held inside the block, the selection was the held block and
/// extending changed nothing on screen.
test("Shift+Down after Ctrl+A grows the region by a row", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.press("Shift+ArrowDown");
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 0, c0: 0, r1: 5, c1: 3 });
});
