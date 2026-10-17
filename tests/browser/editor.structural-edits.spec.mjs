// Structural edits against Excel (XL-FEAT-126, XL-UX-173).
//
// XL-FEAT-126: inserting rows or columns when the last row (column) holds
// something used to push it past row 1,048,576 — the edit succeeded and the
// content left the grid. Excel refuses the insert with "Microsoft Excel can't
// insert new cells because it would push non-empty cells off the end of the
// worksheet…", raised in its message box; the engine now refuses it for every
// path, and the editor shows that message.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    ed.selectForTest(0, 0);
    ed.draw();
  });
  await page.locator("#grid").focus();
}

const api = (page, fn, args = []) =>
  page.evaluate(([body, a]) => new Function("w", "args", body)(window.opencalcEditor.wasmApi(), a), [fn, args]);
const input = (page, r, c) => api(page, "return w.session_cell_input(0, args[0], args[1]);", [r, c]);
const box = (page) => page.locator("#oc-msgbox");

const OFF_GRID =
  "OpenCalc can't insert new cells because it would push non-empty cells off the end of the worksheet. " +
  "These non-empty cells might appear empty but have blank values, some formatting, or a formula. " +
  "Delete enough rows or columns to make room for what you want to insert and then try again.";

test("inserting a row when the last row holds a value is refused with Excel's message", async ({ page }) => {
  await boot(page);
  await api(page, "w.session_set_cell(0, 1048575, 0, 'edge'); w.session_set_cell(0, 1, 0, 'two');");
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 0));
  await page.keyboard.press("Shift+Space");
  await page.keyboard.press("Control+Shift+Equal");
  await expect(box(page)).toBeVisible();
  await expect(page.locator("#oc-msgbox-text")).toHaveText(OFF_GRID);
  expect(await input(page, 1048575, 0), "the last row's value moved or vanished").toBe("edge");
  expect(await input(page, 1, 0), "a row was inserted anyway").toBe("two");
  await page.keyboard.press("Enter");
  await expect(box(page)).toBeHidden();
});

test("inserting a column when column XFD holds a value is refused the same way", async ({ page }) => {
  await boot(page);
  await api(page, "w.session_set_cell(0, 3, 16383, 'edge');");
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 2));
  await page.keyboard.press("Control+Space");
  await page.keyboard.press("Control+Shift+Equal");
  await expect(page.locator("#oc-msgbox-text")).toHaveText(OFF_GRID);
  expect(await input(page, 3, 16383)).toBe("edge");
});
