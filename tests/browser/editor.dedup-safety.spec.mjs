// Remove Duplicates must not delete data the user never selected.
//
// Found by sweeping the editing surface. The engine deletes whole *sheet rows*
// — `EditOperation::DeleteRows` — while the dialog talks only about the
// selected columns and says "rows below shift up". So selecting A1:A6 and
// removing duplicates took column E with it:
//
//     column E before   1 2 3 4 5 6
//     column E after    1 3 5
//     status bar        "removed 3 duplicate rows"
//
// Three values destroyed, off-screen, in a column the user never touched, with
// a status message that reported success. This is the failure mode this project
// names first in its own rules: no silent data loss.
//
// Excel's answer is to notice the adjacent data and offer to widen the
// selection before doing anything.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    // Duplicates in the column the user selects...
    ["x", "x", "y", "y", "z", "z"].forEach((v, r) => a.session_set_cell(0, r, 0, v));
    // ...and untouched data four columns away, off to the right.
    [1, 2, 3, 4, 5, 6].forEach((v, r) => a.session_set_cell(0, r, 4, String(v)));
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
  for (let i = 0; i < 5; i += 1) await page.keyboard.press("Shift+ArrowDown");
}

const colE = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [0, 1, 2, 3, 4, 5].map((r) => a.session_cell_input(0, r, 4));
  });

test("it warns that data outside the selection would be affected", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("data.remove-duplicates"));
  const modal = page.locator(".oc-modal:not([hidden])");
  await expect(modal).toBeVisible();
  // The old wording described only the selected columns, so a user reading it
  // carefully still had no way to know column E was at risk.
  await expect(modal).toContainText(/next to|adjacent|outside|expand|other columns/i);
});

test("cancelling changes nothing at all", async ({ page }) => {
  await boot(page);
  const before = await colE(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("data.remove-duplicates"));
  await expect(page.locator(".oc-modal:not([hidden])")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  expect(await colE(page)).toEqual(before);
});

test("no value outside the selection is destroyed", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("data.remove-duplicates"));
  await expect(page.locator(".oc-modal:not([hidden])")).toBeVisible();
  await page.locator(".oc-modal:not([hidden]) button").last().click();
  await page.waitForTimeout(400);

  // Whatever the user chose, every one of 1..6 must still exist somewhere in
  // column E. Rows may move; values may not vanish.
  const after = (await colE(page)).filter(Boolean);
  for (const v of ["1", "2", "3", "4", "5", "6"]) {
    expect(after, `${v} was destroyed in a column the user never selected`).toContain(v);
  }
});
