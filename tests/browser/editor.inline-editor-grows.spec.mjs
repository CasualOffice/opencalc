// The in-cell editor widens to its text, as Excel's does (UX-SHELL-14).
//
// It was sized to the cell, so a formula longer than the column scrolled
// inside a 64px box and its start disappeared — "SUM(B2:B" with the `=` out
// of sight. Excel's editor grows rightward over the neighbouring cells while
// you type, up to the edge of the window.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

test("a long entry widens the editor and keeps its start in view", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(14, 0, 14, 0)); // A15
  const text = "=SUM(B2:B4)+AVERAGE(C2:C4)+MAX(D2:D4)";
  await page.keyboard.type(text);
  const box = await page.locator("#inline-edit").evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { w: r.width, scrollW: el.scrollWidth, clientW: el.clientWidth, scrollLeft: el.scrollLeft };
  });
  const cellW = await page.evaluate(() => window.opencalcEditor.cellBoxForTest(14, 0).w);
  expect(box.w, "the editor did not grow past its cell").toBeGreaterThan(cellW + 40);
  expect(box.scrollLeft, "the start of the entry scrolled out of the box").toBe(0);
  expect(box.scrollW, "the text still overflows the editor").toBeLessThanOrEqual(box.clientW + 1);
});

test("a short entry keeps the cell's width", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(14, 0, 14, 0));
  await page.keyboard.type("7");
  const w = await page.locator("#inline-edit").evaluate((el) => el.getBoundingClientRect().width);
  const cellW = await page.evaluate(() => window.opencalcEditor.cellBoxForTest(14, 0).w);
  expect(Math.abs(w - cellW)).toBeLessThan(3);
});
