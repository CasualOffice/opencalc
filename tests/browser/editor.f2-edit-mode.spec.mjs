// F2 toggles Point into Edit mode while building a formula (fit-gap XL-UX-003).
//
// In Excel, typing `=SUM(` and pressing an arrow points at a cell; F2 then
// switches to Edit mode, where the arrows move the caret through the text. Here
// F2 did nothing, so the arrows kept inserting references into the middle of
// the formula and corrupted it.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

test("F2 during formula entry stops the arrows from pointing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(9, 0)); // A10
  await page.keyboard.type("=SUM(");
  await page.keyboard.press("ArrowUp");                 // points at A9
  await expect(page.locator("#inline-edit")).toHaveValue("=SUM(A9");
  await page.keyboard.press("F2");
  await expect(page.locator("#cell-mode")).toHaveText("Edit");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expect(page.locator("#inline-edit"), "the arrows still picked cells").toHaveValue("=SUM(A9");
  expect(await page.locator("#inline-edit").evaluate((el) => el.selectionStart)).toBe(5);
});

test("F2 again returns to pointing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(9, 0));
  await page.keyboard.type("=1+");
  await page.keyboard.press("F2");
  await page.keyboard.press("F2");
  await page.keyboard.press("ArrowUp");
  await expect(page.locator("#inline-edit")).toHaveValue("=1+A9");
});
