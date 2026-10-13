// Ctrl+− and Ctrl+Shift+= on cells ask how, as Excel does (fit-gap XL-UX-002/005).
//
// With one cell selected, Ctrl+− deleted the entire row and Ctrl+Shift+=
// inserted one — no question asked, on the most destructive keystroke in the
// grid. Excel opens Delete / Insert with Shift cells up/left (down/right),
// Entire row and Entire column. Whole rows or columns selected still go
// straight through, as in Excel.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const input = (page, r, c) => page.evaluate(([a, b]) => window.opencalcEditor.wasmApi().session_cell_input(0, a, b), [r, c]);

// Column A: no formula refers to it, so no reference warning stands between
// the dialog and the shift (the B column feeds D's totals and would ask).
test("Ctrl+− on a cell asks, and Shift cells up moves only that column", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 0)); // A2 = Widget
  await page.keyboard.press("Control+Minus");
  await expect(page.locator("#oc-modal-title")).toHaveText("Delete");
  expect(await input(page, 1, 1), "the row went before anyone was asked").toBe("3");
  await expect(page.locator('#oc-modal input[value="up"]')).toBeChecked();
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await input(page, 1, 0), "A3 did not move up into A2").toBe("Gadget");
  expect(await input(page, 1, 1), "column B moved too").toBe("3");
});

test("Cancel on the Delete dialog changes nothing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.press("Control+Minus");
  await page.keyboard.press("Escape");
  await expect(page.locator("#oc-modal")).toBeHidden();
  expect(await input(page, 1, 1)).toBe("3");
  expect(await input(page, 1, 0)).toBe("Widget");
});

test("Entire row from the Delete dialog removes the row", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.press("Control+Minus");
  await page.locator('#oc-modal input[value="row"]').check();
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await input(page, 1, 0)).toBe("Gadget");
});

test("Ctrl+Shift+= on a cell asks, and Shift cells down inserts one cell", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 0));
  await page.keyboard.press("Control+Shift+Equal");
  await expect(page.locator("#oc-modal-title")).toHaveText("Insert");
  await expect(page.locator('#oc-modal input[value="down"]')).toBeChecked();
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await input(page, 1, 0), "A2 was not emptied").toBe("");
  expect(await input(page, 2, 0), "the old A2 did not move down").toBe("Widget");
  expect(await input(page, 1, 1), "column B moved too").toBe("3");
});

test("whole rows selected still delete without a dialog", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.press("Shift+Space");
  await page.keyboard.press("Control+Minus");
  await expect(page.locator("#oc-modal")).toBeHidden();
  expect(await input(page, 1, 0)).toBe("Gadget");
});
