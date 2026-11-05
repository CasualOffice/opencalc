// Format Cells has Excel's six tabs in Excel's order, Protection among them
// (fit-gap XL-UX-044).
//
// Excel: Number, Alignment, Font, Border, Fill, Protection. Protection has two
// checkboxes, Locked and Hidden, under the sentence "Locking cells or hiding
// formulas has no effect until you protect the worksheet (Review tab, Protect
// group, Protect Sheet button)." — and a box that is ticked for some of the
// selected cells and not others shows as indeterminate, and applying the
// dialog leaves those cells as they were. [unverified] wording from memory of
// Excel 365.
//
// Ours had five tabs as Number, Font, Alignment, Fill, Border, and Locked /
// Hide formula lived only on the Format menu.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const api = (page, fn, arg) => page.evaluate(fn, arg);
const prot = (page, r, c) => api(page, ([r, c]) =>
  JSON.parse(window.opencalcEditor.wasmApi().session_cell_protection(0, r, c)), [r, c]);

test("the tabs are Excel's six, in Excel's order", async ({ page }) => {
  await boot(page);
  await api(page, () => window.opencalcEditor.formatCellsDialog());
  await expect(page.locator(".fc-tabs .fc-tab")).toHaveText(["Number", "Alignment", "Font", "Border", "Fill", "Protection"]);
});

test("Protection: Locked and Hidden, with Excel's note, applied on OK", async ({ page }) => {
  await boot(page);
  await api(page, () => {
    window.opencalcEditor.wasmApi().session_set_cell(0, 1, 1, "=1");
    window.opencalcEditor.selectForTest(1, 1);
    window.opencalcEditor.formatCellsDialog();
  });
  await page.locator(".fc-tabs .fc-tab", { hasText: "Protection" }).click();
  const locked = page.getByLabel("Locked", { exact: true });
  const hidden = page.getByLabel("Hidden", { exact: true });
  await expect(locked).toBeChecked();
  await expect(hidden).not.toBeChecked();
  await expect(page.locator(".fc-page:not([hidden])")).toContainText(
    "Locking cells or hiding formulas has no effect until you protect the worksheet (Review tab, Protect group, Protect Sheet button).");
  await locked.uncheck();
  await hidden.check();
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await prot(page, 1, 1)).toEqual({ locked: false, hidden: true });
});

test("a mixed selection shows indeterminate and is left mixed by OK", async ({ page }) => {
  await boot(page);
  await api(page, () => {
    const e = window.opencalcEditor;
    const a = e.wasmApi();
    a.session_set_cell(0, 0, 0, "a");
    a.session_set_cell(0, 1, 0, "b");
    a.session_set_cell_protection(0, 0, 0, 0, 0, "locked", false);
    e.selectForTest(0, 0);
    e.extendSelectionForTest(1, 0);
    e.formatCellsDialog();
  });
  await page.locator(".fc-tabs .fc-tab", { hasText: "Protection" }).click();
  const indeterminate = await page.getByLabel("Locked", { exact: true }).evaluate((b) => b.indeterminate);
  expect(indeterminate, "a mixed Locked box is not indeterminate").toBe(true);
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect((await prot(page, 0, 0)).locked).toBe(false);
  expect((await prot(page, 1, 0)).locked).toBe(true);
});
