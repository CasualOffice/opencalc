// Excel's Insert Function and Function Arguments dialogs (fit-gap
// XL-UX-026/027).
//
// Shift+F3, the fx button by the formula bar and Formulas ▸ Insert Function
// open Insert Function: a search box with Go, "Or select a category", the
// function list, and the selected function's syntax and description. OK goes
// on to Function Arguments: one box per argument with its description and live
// value, the formula result, and OK writing the formula into the cell.
// OpenCalc had a flat A–Z list with a search box and dropped `=NAME()` into
// the cell — no categories, no descriptions, no arguments step.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "1");
    a.session_set_cell(0, 1, 0, "2");
    a.session_set_cell(0, 2, 0, "3");
    window.opencalcEditor.selectForTest(5, 2);
  });
  await page.locator("#grid").focus();
}
const title = (page) => page.locator("#oc-modal-title");
const list = (page) => page.locator("#oc-fn-list .fn-row");
const ok = (page) => page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary");
const insert = (page) => page.evaluate(() => window.opencalcEditor.runCommand("formulas.insert-function"));

const CATEGORIES = [
  "Most Recently Used", "All", "Financial", "Date & Time", "Math & Trig", "Statistical",
  "Lookup & Reference", "Database", "Text", "Logical", "Information", "Engineering",
  "Cube", "Compatibility", "Web",
];

for (const [how, open] of [
  ["Shift+F3", (p) => p.keyboard.press("Shift+F3")],
  ["the fx button", (p) => p.locator("#fx-insert").click()],
  ["Formulas ▸ Insert Function", insert],
]) {
  test(`${how} opens Insert Function`, async ({ page }) => {
    await boot(page);
    await open(page);
    await expect(title(page)).toHaveText("Insert Function");
    await expect(page.locator("#oc-fn-search")).toBeVisible();
    await expect(page.locator("#oc-fn-go")).toHaveText("Go");
    await expect(page.locator("#oc-fn-cat option")).toHaveText(CATEGORIES);
    await expect(page.locator("#oc-fn-cat")).toHaveValue("Most Recently Used");
    await expect(list(page).first()).toHaveText("SUM");
  });
}

test("every implemented function has a category and a description, and only those are listed", async ({ page }) => {
  await boot(page);
  const gaps = await page.evaluate(async () => {
    const { FN_INFO } = await import("/fn-info.js");
    const cat = JSON.parse(window.opencalcEditor.wasmApi().function_catalog()).map((f) => f.n);
    return {
      missing: cat.filter((n) => !FN_INFO[n] || !FN_INFO[n][1]),
      extra: Object.keys(FN_INFO).filter((n) => !cat.includes(n)),
      count: cat.length,
    };
  });
  expect(gaps.missing).toEqual([]);
  expect(gaps.extra, "a function the engine does not implement is offered").toEqual([]);
  await insert(page);
  await page.locator("#oc-fn-cat").selectOption("All");
  await expect(list(page)).toHaveCount(gaps.count);
});

test("a category lists its functions, and the selection shows syntax and description", async ({ page }) => {
  await boot(page);
  await insert(page);
  await page.locator("#oc-fn-cat").selectOption("Lookup & Reference");
  await expect(list(page).filter({ hasText: /^VLOOKUP$/ })).toHaveCount(1);
  await expect(list(page).filter({ hasText: /^SUM$/ })).toHaveCount(0);
  await list(page).filter({ hasText: /^VLOOKUP$/ }).click();
  await expect(page.locator("#oc-fn-syntax")).toHaveText("VLOOKUP(lookup_value,table_array,col_index_num,range_lookup)");
  await expect(page.locator("#oc-fn-desc")).toContainText("Looks for a value in the leftmost column of a table");
  await page.locator("#oc-fn-cat").selectOption("Math & Trig");
  await list(page).filter({ hasText: /^SUM$/ }).click();
  await expect(page.locator("#oc-fn-syntax")).toHaveText("SUM(number1,number2,...)");
  await expect(page.locator("#oc-fn-desc")).toHaveText("Adds all the numbers in a range of cells.");
});

test("Search and Go recommend matching functions", async ({ page }) => {
  await boot(page);
  await insert(page);
  await page.locator("#oc-fn-search").fill("lookup");
  await page.locator("#oc-fn-go").click();
  await expect(page.locator("#oc-fn-cat")).toHaveValue("Recommended");
  await expect(list(page).filter({ hasText: /^XLOOKUP$/ })).toHaveCount(1);
  await expect(list(page).filter({ hasText: /^VLOOKUP$/ })).toHaveCount(1);
});

test("OK goes to Function Arguments, which shows live values and writes the formula", async ({ page }) => {
  await boot(page);
  await insert(page);
  await list(page).filter({ hasText: /^SUM$/ }).click();
  await ok(page).click();
  await expect(title(page)).toHaveText("Function Arguments");
  await expect(page.locator(".oc-fa-name")).toHaveText(["Number1", "Number2"]);
  await expect(page.locator("#oc-fa-fdesc")).toHaveText("Adds all the numbers in a range of cells.");
  const box = page.locator(".oc-fa-input");
  await expect(box.first()).toBeFocused();
  await expect(page.locator("#oc-fa-adesc")).toContainText("Number1:");
  await box.first().fill("A1:A3");
  await expect(page.locator(".oc-fa-value").first()).toHaveText("= {1;2;3}");
  await expect(page.locator("#oc-fa-result")).toHaveText("Formula result = 6");
  await box.nth(1).fill("10");
  await expect(page.locator(".oc-fa-name"), "a repeating argument grows a box").toHaveText(["Number1", "Number2", "Number3"]);
  await expect(page.locator("#oc-fa-result")).toHaveText("Formula result = 16");
  await ok(page).click();
  await expect(page.locator("#oc-modal")).toBeHidden();
  const cell = await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 5, 2));
  expect(cell).toBe("=SUM(A1:A3,10)");
});

test("Function Arguments leaves out trailing empty optional arguments", async ({ page }) => {
  await boot(page);
  await insert(page);
  await page.locator("#oc-fn-cat").selectOption("Math & Trig");
  await list(page).filter({ hasText: /^ROUND$/ }).click();
  await ok(page).click();
  await expect(page.locator(".oc-fa-name")).toHaveText(["Number", "Num_digits"]);
  await page.locator(".oc-fa-input").first().fill("2.567");
  await page.locator(".oc-fa-input").nth(1).fill("1");
  await expect(page.locator("#oc-fa-result")).toHaveText("Formula result = 2.6");
  await page.keyboard.press("Enter");
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 5, 2))).toBe("=ROUND(2.567,1)");
});

test("Cancel writes nothing", async ({ page }) => {
  await boot(page);
  await insert(page);
  await ok(page).click();
  await page.keyboard.press("Escape");
  await expect(page.locator("#oc-modal")).toBeHidden();
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 5, 2))).toBe("");
});
