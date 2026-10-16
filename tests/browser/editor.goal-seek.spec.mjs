// Goal Seek (fit-gap XL-FEAT-032).
//
// Excel: Data ▸ Forecast ▸ What-If Analysis ▸ Goal Seek. Set cell (a formula),
// To value, By changing cell (a constant) → the engine searches for the input,
// then a Goal Seek Status step reports it; OK keeps it, Cancel puts the input
// back. The search must not edit the workbook trial by trial — no undo flood,
// no collaboration traffic — so the answer is committed once, on OK, as one
// undo step. Refusals are Excel's messages, shown in the dialog (docs/95
// rule 8).

import { expect, test } from "@playwright/test";

async function boot(page, cells) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate((cells) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (const [r, c, v] of cells) a.session_set_cell(0, r, c, v);
    window.opencalcEditor.selectForTest(0, 1);
  }, cells);
  await page.locator("#grid").focus();
}

const SQUARE = [[0, 0, "1"], [0, 1, "=A1^2"]]; // A1 = 1, B1 = A1^2
const modal = (page) => page.locator("#oc-modal");
const title = (page) => page.locator("#oc-modal-title");
const setCell = (page) => page.getByLabel("Set cell:", { exact: true });
const toValue = (page) => page.getByLabel("To value:", { exact: true });
const byChanging = (page) => page.getByLabel("By changing cell:", { exact: true });
const btn = (page, name) => page.locator("#oc-modal-body").getByRole("button", { name, exact: true });
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const value = (page, ref) => page.evaluate((ref) => window.opencalcEditor.wasmApi().session_evaluate(0, ref), ref);
const openGoalSeek = (page) => page.evaluate(() => window.opencalcEditor.runCommand("data.goal-seek"));

async function fill(page, set, to, change) {
  await setCell(page).fill(set);
  await toValue(page).fill(to);
  await byChanging(page).fill(change);
  await btn(page, "OK").click();
}

test("the Data tab carries What-If Analysis ▸ Goal Seek", async ({ page }) => {
  await boot(page, SQUARE);
  await page.locator('.oc-ribbon [role="tab"]', { hasText: "Data" }).click();
  const whatIf = page.locator('.oc-ribbon [data-cmd="data.goal-seek"]').first();
  await expect(whatIf).toBeVisible();
  await expect(page.locator(".oc-ribbon")).toContainText("Forecast");
});

test("Goal Seek opens with Excel's three fields, Set cell on the active cell", async ({ page }) => {
  await boot(page, SQUARE);
  await openGoalSeek(page);
  await expect(modal(page)).toBeVisible();
  await expect(title(page)).toHaveText("Goal Seek");
  await expect(setCell(page)).toHaveValue("B1");
  await expect(setCell(page)).toBeFocused();
  await expect(toValue(page)).toHaveValue("");
  await expect(byChanging(page)).toHaveValue("");
});

test("it finds √2 for =A1^2 → 2 and reports it before anything is changed", async ({ page }) => {
  await boot(page, SQUARE);
  await openGoalSeek(page);
  await fill(page, "B1", "2", "A1");
  await expect(title(page)).toHaveText("Goal Seek Status");
  const body = page.locator("#oc-modal-body");
  await expect(body).toContainText("Goal Seeking with Cell B1");
  await expect(body).toContainText("found a solution.");
  await expect(page.locator("#oc-gs-target")).toHaveText("2");
  // Within Excel's Maximum Change of the target, as the set cell shows it.
  const current = Number(await page.locator("#oc-gs-current").textContent());
  expect(Math.abs(current - 2)).toBeLessThanOrEqual(0.001);
  // The status is up and the workbook is untouched: the search ran on a copy.
  expect(await input(page, 0, 0)).toBe("1");
  await btn(page, "OK").click();
  await expect(modal(page)).toBeHidden();
  const kept = Number(await input(page, 0, 0));
  expect(Math.abs(kept - Math.SQRT2)).toBeLessThan(1e-3);
  expect(Math.abs(Number(await value(page, "B1")) - 2)).toBeLessThanOrEqual(0.001);
  // One undo step puts it back — the search made none of its own.
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+z");
  await expect.poll(() => input(page, 0, 0)).toBe("1");
  expect(await input(page, 0, 1)).toBe("=A1^2");
  expect(await value(page, "B1")).toBe("1");
});

test("Cancel on the status leaves the input as it was", async ({ page }) => {
  await boot(page, SQUARE);
  await openGoalSeek(page);
  await fill(page, "B1", "9", "A1");
  await expect(title(page)).toHaveText("Goal Seek Status");
  await btn(page, "Cancel").click();
  await expect(modal(page)).toBeHidden();
  expect(await input(page, 0, 0)).toBe("1");
});

test("no solution says it may not have found one, with the closest value", async ({ page }) => {
  await boot(page, [[0, 0, "2"], [0, 1, "=A1^2+1"]]);
  await openGoalSeek(page);
  await fill(page, "B1", "0", "A1");
  await expect(title(page)).toHaveText("Goal Seek Status");
  await expect(page.locator("#oc-modal-body")).toContainText("may not have found a solution.");
  await expect(page.locator("#oc-gs-target")).toHaveText("0");
  const current = Number(await page.locator("#oc-gs-current").textContent());
  expect(current).toBeGreaterThanOrEqual(1);
  expect(current).toBeLessThan(1.1);
});

for (const [why, set, to, change, message] of [
  ["Set cell holds a constant", "A1", "2", "A1", "Cell must contain a formula."],
  ["By changing cell holds a formula", "B1", "2", "B1", "Cell must contain a value."],
  ["To value is not a number", "B1", "two", "A1", "Invalid value."],
  ["Set cell is not a reference", "B1:B2", "2", "A1", "Reference must be to a single cell."],
  ["By changing cell is not a reference", "B1", "2", "nowhere!!", "Reference is not valid."],
  ["a cell on another sheet", "B1", "2", "Sheet2!A1", "Reference must be on the active sheet."],
]) {
  test(`refused inline when ${why}`, async ({ page }) => {
    await boot(page, SQUARE);
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_add_sheet());
    await openGoalSeek(page);
    await fill(page, set, to, change);
    await expect(page.locator("#oc-gs-error")).toHaveText(message);
    // Still the Goal Seek dialog, with what was typed, and nothing changed.
    await expect(title(page)).toHaveText("Goal Seek");
    await expect(setCell(page)).toHaveValue(set);
    expect(await input(page, 0, 0)).toBe("1");
    expect(await page.locator(".oc-modal-box:visible").count()).toBe(1);
  });
}
