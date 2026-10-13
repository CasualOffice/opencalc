// Excel's Go To Special (fit-gap XL-UX-020).
//
// Reached from Go To's Special… button and from Home ▸ Find & Select ▸ Go To
// Special. It selects the matching cells — inside the selection when that is
// more than one cell, else inside the used range — as a multi-range
// selection, and says "No cells were found." when nothing matches. OpenCalc
// had no such command at all.

import { expect, test } from "@playwright/test";

///        A      B        C
///   1    1      =A1*2    x
///   2    2      =A2*2
///   3    TRUE   =1/0     y
async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (const [r, c, v] of [
      [0, 0, "1"], [1, 0, "2"], [2, 0, "TRUE"],
      [0, 1, "=A1*2"], [1, 1, "=A2*2"], [2, 1, "=1/0"],
      [0, 2, "x"], [2, 2, "y"],
    ]) a.session_set_cell(0, r, c, v);
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}
const ranges = (page) => page.evaluate(() => window.opencalcEditor.allRanges()
  .map(({ r0, c0, r1, c1 }) => ({ r0, c0, r1, c1 }))
  .sort((a, b) => a.r0 - b.r0 || a.c0 - b.c0));
const box = (r0, c0, r1 = r0, c1 = c0) => ({ r0, c0, r1, c1 });
const openSpecial = (page) => page.evaluate(() => window.opencalcEditor.runCommand("edit.go-to-special"));
const pick = async (page, label) => {
  await page.locator("#oc-modal .oc-radio", { hasText: new RegExp(`^${label}$`) }).locator("input").check();
};
const ok = (page) => page.locator("#oc-modal .oc-btn.primary").click();

test("Go To's Special… button opens Go To Special", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await page.locator("#oc-modal button", { hasText: "Special…" }).click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Go To Special");
});

test("Home ▸ Find & Select lists Go To Special", async ({ page }) => {
  await boot(page);
  await page.locator("#rb-tab-home").click();
  await page.locator('.oc-ribbon [data-cmd="edit.find-replace"]').click();
  await page.locator('.rb-pop-item[data-cmd="edit.go-to-special"]').click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Go To Special");
});

test("Constants with one cell selected searches the used range", async ({ page }) => {
  await boot(page);
  await openSpecial(page);
  await pick(page, "Constants");
  await ok(page);
  await expect(page.locator("#oc-modal")).toBeHidden();
  expect(await ranges(page)).toEqual([box(0, 0, 2, 0), box(0, 2), box(2, 2)]);
});

test("Formulas filtered to Errors finds the #DIV/0!", async ({ page }) => {
  await boot(page);
  await openSpecial(page);
  await pick(page, "Formulas");
  for (const t of ["Numbers", "Text", "Logicals"]) {
    await page.locator("#oc-modal .oc-check", { hasText: t }).locator("input").uncheck();
  }
  await ok(page);
  expect(await ranges(page)).toEqual([box(2, 1)]);
});

test("Blanks finds the hole in the used range", async ({ page }) => {
  await boot(page);
  await openSpecial(page);
  await pick(page, "Blanks");
  await ok(page);
  expect(await ranges(page)).toEqual([box(1, 2)]);
});

test("a multi-cell selection limits the search to itself", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.extendSelectionForTest(1, 1);   // A1:B2
  });
  await openSpecial(page);
  await pick(page, "Constants");
  await ok(page);
  expect(await ranges(page)).toEqual([box(0, 0, 1, 0)]);
});

test("nothing found says so and keeps the selection", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.extendSelectionForTest(2, 0);   // A1:A3, all constants
  });
  await openSpecial(page);
  await pick(page, "Formulas");
  await ok(page);
  await expect(page.locator("#oc-gts-error")).toHaveText("No cells were found.");
  expect(await ranges(page)).toEqual([box(0, 0, 2, 0)]);
});

test("Current region and Last cell", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await openSpecial(page);
  await pick(page, "Current region");
  await ok(page);
  expect(await ranges(page)).toEqual([box(0, 0, 2, 2)]);

  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await openSpecial(page);
  await pick(page, "Last cell");
  await ok(page);
  expect(await ranges(page)).toEqual([box(2, 2)]);
  await expect(page.locator("#cell-ref")).toHaveValue("C3");
});

test("Visible cells only skips a hidden row", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_hide_rows(0, 1, 1);
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.extendSelectionForTest(2, 2);   // A1:C3
  });
  await openSpecial(page);
  await pick(page, "Visible cells only");
  await ok(page);
  expect(await ranges(page)).toEqual([box(0, 0, 0, 2), box(2, 0, 2, 2)]);
});

test("Notes selects the cells that have one", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_comment(0, 2, 2, "check", "", ""));
  await openSpecial(page);
  await pick(page, "Notes");
  await ok(page);
  expect(await ranges(page)).toEqual([box(2, 2)]);
});
