// Find and Replace as Excel's dialog (fit-gap XL-UX-024/025).
//
// Ctrl+F and Ctrl+H used to open an 852×44 floating bar at the grid's top
// right whose options were five unlabelled icon checkboxes. Excel opens a
// **Find and Replace** dialog with Find and Replace tabs, an **Options >>**
// that reveals labelled controls (Within, Search, Look in, Match case, Match
// entire cell contents), Find All with a results list, and messages about the
// search shown in the dialog (docs/95 rule 8: inline, never a box over it).
// The dialog is modeless, as Excel's is: the grid stays usable underneath.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "apple");     // A1
    a.session_set_cell(0, 0, 2, "Apple pie");  // C1
    a.session_set_cell(0, 2, 0, "pineapple");  // A3
    a.session_set_cell(0, 1, 1, "banana");     // B2
    a.session_set_cell(0, 4, 1, "=LEN(A1)");   // B5 → 5
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}

const modal = (page) => page.locator("#oc-modal");
const what = (page) => page.locator("#oc-fr-what");
const withField = (page) => page.locator("#oc-fr-with");
const msg = (page) => page.locator("#oc-fr-msg");
const btn = (page, name) => page.locator("#oc-modal-body").getByRole("button", { name, exact: true });
const nameBox = (page) => page.locator("#cell-ref");
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);

test("Ctrl+F opens the Find and Replace dialog on its Find tab", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await expect(modal(page)).toBeVisible();
  await expect(page.locator("#oc-modal-title")).toHaveText("Find and Replace");
  await expect(page.getByRole("tab", { name: "Find", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(what(page)).toBeFocused();
  await expect(withField(page)).toBeHidden();
  // The floating bar is gone, not merely hidden behind the dialog.
  await expect(page.locator("#find-bar")).toHaveCount(0);
  // Find tab's buttons, in Excel's order.
  const names = await page.locator("#oc-modal .oc-confirm-actions button:visible").allTextContents();
  expect(names.map((s) => s.trim())).toEqual(["Find All", "Find Next", "Close"]);
});

test("Options >> reveals Excel's labelled options", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await expect(page.getByLabel("Within:")).toBeHidden();
  await btn(page, "Options >>").click();
  await expect(btn(page, "Options <<")).toBeVisible();
  for (const [label, options] of [
    ["Within:", ["Sheet", "Workbook"]],
    ["Search:", ["By Rows", "By Columns"]],
    ["Look in:", ["Formulas", "Values", "Notes"]],
  ]) {
    const select = page.getByLabel(label, { exact: true });
    await expect(select).toBeVisible();
    expect((await select.locator("option").allTextContents()).map((s) => s.trim())).toEqual(options);
  }
  await expect(page.getByLabel("Match case", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Match entire cell contents", { exact: true })).toBeVisible();
});

test("Ctrl+H opens the Replace tab, whose Look in offers Formulas only", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+h");
  await expect(page.locator("#oc-modal-title")).toHaveText("Find and Replace");
  await expect(page.getByRole("tab", { name: "Replace", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(withField(page)).toBeVisible();
  const names = await page.locator("#oc-modal .oc-confirm-actions button:visible").allTextContents();
  expect(names.map((s) => s.trim())).toEqual(["Replace All", "Replace", "Find All", "Find Next", "Close"]);
  await btn(page, "Options >>").click();
  expect((await page.getByLabel("Look in:", { exact: true }).locator("option").allTextContents()).map((s) => s.trim()))
    .toEqual(["Formulas"]);
});

test("Find Next walks the matches from the active cell, by rows, and wraps", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("apple");
  // From A1 the next match by rows is C1, then A3, then round to A1.
  for (const ref of ["C1", "A3", "A1", "C1"]) {
    await btn(page, "Find Next").click();
    await expect(nameBox(page)).toHaveValue(ref);
  }
  // Enter in the field is Find Next too.
  await what(page).press("Enter");
  await expect(nameBox(page)).toHaveValue("A3");
});

test("By Columns changes the order Find Next walks", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("apple");
  await btn(page, "Options >>").click();
  await page.getByLabel("Search:", { exact: true }).selectOption({ label: "By Columns" });
  await btn(page, "Find Next").click();
  await expect(nameBox(page)).toHaveValue("A3");
  await btn(page, "Find Next").click();
  await expect(nameBox(page)).toHaveValue("C1");
});

test("Match case and Match entire cell contents narrow the search", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("apple");
  await btn(page, "Options >>").click();
  await page.getByLabel("Match entire cell contents", { exact: true }).check();
  await btn(page, "Find All").click();
  await expect(page.locator("#oc-fr-count")).toHaveText("1 cell(s) found");
  await page.getByLabel("Match entire cell contents", { exact: true }).uncheck();
  await page.getByLabel("Match case", { exact: true }).check();
  await what(page).fill("Apple");
  await btn(page, "Find All").click();
  await expect(page.locator("#oc-fr-count")).toHaveText("1 cell(s) found");
});

test("Look in Values finds what a formula shows, not what it says", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("5");
  await btn(page, "Find All").click();
  await expect(msg(page)).toHaveText("We couldn't find what you were looking for.");
  await btn(page, "Options >>").click();
  await page.getByLabel("Look in:", { exact: true }).selectOption({ label: "Values" });
  await btn(page, "Find All").click();
  await expect(page.locator("#oc-fr-count")).toHaveText("1 cell(s) found");
});

test("Find All lists Book, Sheet, Name, Cell, Value and Formula, and a row selects its cell", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("apple");
  await btn(page, "Find All").click();
  const table = page.locator("#oc-fr-results");
  await expect(table).toBeVisible();
  expect((await table.locator("thead th").allTextContents()).map((s) => s.trim()))
    .toEqual(["Book", "Sheet", "Name", "Cell", "Value", "Formula"]);
  await expect(table.locator("tbody tr")).toHaveCount(3);
  await expect(page.locator("#oc-fr-count")).toHaveText("3 cell(s) found");
  const third = table.locator("tbody tr").nth(2);
  await expect(third.locator("td").nth(3)).toHaveText("$A$3");
  await expect(third.locator("td").nth(4)).toHaveText("pineapple");
  await third.click();
  await expect(nameBox(page)).toHaveValue("A3");
  await expect(modal(page)).toBeVisible();
});

test("nothing found is said inline and the dialog stays open with the text", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("durian");
  await btn(page, "Find Next").click();
  await expect(msg(page)).toHaveText("We couldn't find what you were looking for.");
  await expect(modal(page)).toBeVisible();
  await expect(what(page)).toHaveValue("durian");
  await expect(page.locator(".oc-modal-box:visible")).toHaveCount(1);
});

test("Within: Workbook finds a match on another sheet and goes there", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_add_sheet();
    a.session_set_cell(1, 6, 3, "kiwi");
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.keyboard.press("Control+f");
  await what(page).fill("kiwi");
  await btn(page, "Find Next").click();
  await expect(msg(page)).toHaveText("We couldn't find what you were looking for.");
  await btn(page, "Options >>").click();
  await page.getByLabel("Within:", { exact: true }).selectOption({ label: "Workbook" });
  await btn(page, "Find Next").click();
  await expect(nameBox(page)).toHaveValue("D7");
  await expect(page.locator("#sheet-tabs .sheet-tab").nth(1)).toHaveClass(/active/);
});

test("Replace All reports inline and is one undo step", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+h");
  await what(page).fill("apple");
  await withField(page).fill("pear");
  await btn(page, "Replace All").click();
  await expect(msg(page)).toHaveText("All done. We made 3 replacements.");
  await expect(modal(page)).toBeVisible();
  expect(await input(page, 0, 0)).toBe("pear");
  expect(await input(page, 0, 2)).toBe("pear pie");
  expect(await input(page, 2, 0)).toBe("pinepear");
  await btn(page, "Close").click();
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+z");
  await expect.poll(() => input(page, 0, 0)).toBe("apple");
  expect(await input(page, 0, 2)).toBe("Apple pie");
  expect(await input(page, 2, 0)).toBe("pineapple");
});

test("Replace with nothing to replace says so inline", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+h");
  await what(page).fill("durian");
  await withField(page).fill("x");
  await btn(page, "Replace All").click();
  await expect(msg(page)).toHaveText("We couldn't find anything to replace. Click Options for more ways to search.");
});

test("Replace rewrites the active match and moves to the next", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+h");
  await what(page).fill("apple");
  await withField(page).fill("pear");
  // The active cell A1 matches: Replace rewrites it and goes on to C1.
  await btn(page, "Replace").click();
  expect(await input(page, 0, 0)).toBe("pear");
  await expect(nameBox(page)).toHaveValue("C1");
  expect(await input(page, 0, 2)).toBe("Apple pie");
});

test("the dialog is modeless: the grid takes clicks while it is open", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await expect(modal(page)).toBeVisible();
  const grid = await page.locator("#grid").boundingBox();
  // Bottom-left of the grid, well away from a centred dialog.
  await page.mouse.click(grid.x + 60, grid.y + grid.height - 40);
  await expect(nameBox(page)).not.toHaveValue("A1");
  await expect(modal(page)).toBeVisible();
  // Typing now goes to the grid, not the dialog.
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("grid");
});

test("the title bar drags the dialog", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  const box = page.locator("#oc-modal .oc-modal-box");
  const before = await box.boundingBox();
  const title = await page.locator("#oc-modal-title").boundingBox();
  await page.mouse.move(title.x + 20, title.y + 5);
  await page.mouse.down();
  await page.mouse.move(title.x + 20 - 120, title.y + 5 + 60, { steps: 4 });
  await page.mouse.up();
  const after = await box.boundingBox();
  expect(Math.round(after.x - before.x)).toBe(-120);
  expect(Math.round(after.y - before.y)).toBe(60);
});

test("Shift+F4 repeats the last search after the dialog is closed", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await what(page).fill("apple");
  await btn(page, "Find Next").click();
  await expect(nameBox(page)).toHaveValue("C1");
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeHidden();
  await page.locator("#grid").focus();
  await page.keyboard.press("Shift+F4");
  await expect(nameBox(page)).toHaveValue("A3");
});

test("Close and Escape close it, and another dialog takes the frame back modal", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeHidden();
  await page.keyboard.press("Control+f");
  await btn(page, "Close").click();
  await expect(modal(page)).toBeHidden();
  // Open Find, then Go To over it: Go To is modal again (its backdrop is back).
  await page.keyboard.press("Control+f");
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+g");
  await expect(page.locator("#oc-modal-title")).toHaveText("Go To");
  expect(await page.evaluate(() => getComputedStyle(document.querySelector("#oc-modal")).pointerEvents)).not.toBe("none");
});
