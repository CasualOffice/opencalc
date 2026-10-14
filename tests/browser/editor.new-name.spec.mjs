// Excel's New Name / Edit Name dialog (fit-gap XL-UX-022).
//
// Formulas ▸ Define Name, and New… / Edit… in the Name Manager, open a dialog
// with Name, Scope (Workbook or a sheet), Comment and Refers to — the last
// prefilled with the selection as an absolute, sheet-qualified reference. An
// invalid or duplicate name is refused in Excel's words and the dialog stays
// open. OpenCalc's Define Name opened the Name Manager popover, which could
// not define anything.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    if (JSON.parse(a.session_sheet_names()).length < 2) a.session_add_sheet();
    const names = JSON.parse(a.session_sheet_names());
    window.__sheets = names;
    window.__q = names.map((n) => (/[^A-Za-z0-9_.]/.test(n) ? `'${n.replace(/'/g, "''")}'` : n));
    window.opencalcEditor.selectForTest(1, 1);
  });
  await page.locator("#grid").focus();
}
const modal = (page) => page.locator("#oc-modal");
const title = (page) => page.locator("#oc-modal-title");
const field = (page, which) => page.locator(`#oc-name-${which}`);
const names = (page) => page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_names()));
const ok = (page) => page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary");

const INVALID = "The name that you entered is not valid.";
const DUPLICATE = "The name that you entered already exists. Enter a unique name.";

test("Define Name opens New Name with the selection as an absolute reference", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Shift+ArrowRight");
  await page.keyboard.press("Shift+ArrowDown");
  await page.keyboard.press("Shift+ArrowDown");
  await page.evaluate(() => window.opencalcEditor.runCommand("formulas.define-name"));
  await expect(modal(page)).toBeVisible();
  await expect(title(page)).toHaveText("New Name");
  const q = await page.evaluate(() => window.__q);
  const sheets = await page.evaluate(() => window.__sheets);
  await expect(field(page, "ref")).toHaveValue(`=${q[0]}!$B$2:$C$4`);
  await expect(field(page, "scope").locator("option")).toHaveText(["Workbook", ...sheets]);
  await expect(field(page, "scope")).toHaveValue("-1");
  await expect(field(page, "comment")).toHaveValue("");
  await expect(field(page, "name")).toBeFocused();
  await expect(ok(page)).toHaveText("OK");
});

test("OK defines the name with its scope and comment", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("formulas.define-name"));
  await field(page, "name").fill("TaxRate");
  await field(page, "scope").selectOption({ index: 2 });
  await field(page, "comment").fill("Standard VAT");
  await ok(page).click();
  await expect(modal(page)).toBeHidden();
  const q = await page.evaluate(() => window.__q);
  const sheets = await page.evaluate(() => window.__sheets);
  expect(await names(page)).toEqual([
    { name: "TaxRate", refersTo: `${q[0]}!$B$2`, sheet: 1, scope: sheets[1], comment: "Standard VAT" },
  ]);
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("grid");
});

for (const bad of ["my name", "A1", "R1C1", "1st", "C", "Sales-Tax"]) {
  test(`"${bad}" is refused in Excel's words and the dialog stays open`, async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.runCommand("formulas.define-name"));
    await field(page, "name").fill(bad);
    await page.keyboard.press("Enter");
    await expect(page.locator("#oc-name-error")).toContainText(INVALID);
    await expect(modal(page)).toBeVisible();
    await expect(field(page, "name")).toHaveValue(bad);
    expect(await names(page)).toEqual([]);
  });
}

test("a name is unique within its scope, not across scopes", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_name("", -1, "Rate", -1, "1", ""));
  await page.evaluate(() => window.opencalcEditor.runCommand("formulas.define-name"));
  await field(page, "name").fill("rate");
  await ok(page).click();
  await expect(page.locator("#oc-name-error")).toHaveText(DUPLICATE);
  await expect(modal(page)).toBeVisible();
  await field(page, "scope").selectOption({ index: 1 });
  await ok(page).click();
  await expect(modal(page)).toBeHidden();
  expect((await names(page)).map((n) => [n.name, n.sheet])).toEqual([["Rate", -1], ["rate", 0]]);
});

test("Name Manager ▸ New… defines a name and returns to the Name Manager", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  await page.locator("#oc-nm-new").click();
  await expect(title(page)).toHaveText("New Name");
  await field(page, "name").fill("Fresh");
  await page.keyboard.press("Enter");
  await expect(title(page)).toHaveText("Name Manager");
  await expect(page.locator("#oc-nm-list .oc-nm-row.on [data-col='name']")).toHaveText("Fresh");
  await page.locator("#oc-nm-new").click();
  await page.keyboard.press("Escape");
  await expect(title(page), "Cancel goes back too").toHaveText("Name Manager");
});

test("Edit… renames a name, carries its formulas, and cannot change its scope", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 9, "21");
    a.session_set_name("", -1, "Rate", -1, `${window.__q[0]}!$J$1`, "old note");
    a.session_set_cell(0, 2, 9, "=Rate*2");
  });
  await page.keyboard.press("Control+F3");
  await page.locator("#oc-nm-list .oc-nm-row").first().click();
  await page.locator("#oc-nm-edit").click();
  await expect(title(page)).toHaveText("Edit Name");
  await expect(field(page, "name")).toHaveValue("Rate");
  await expect(field(page, "comment")).toHaveValue("old note");
  await expect(field(page, "scope")).toBeDisabled();
  await field(page, "name").fill("Vat");
  await field(page, "comment").fill("new note");
  await ok(page).click();
  await expect(title(page)).toHaveText("Name Manager");
  await expect(page.locator("#oc-nm-list .oc-nm-row [data-col='name']")).toHaveText(["Vat"]);
  await expect(page.locator("#oc-nm-list .oc-nm-row [data-col='comment']")).toHaveText(["new note"]);
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 2, 9))).toBe("=Vat*2");
});

test("Cancel and Escape define nothing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("formulas.define-name"));
  await field(page, "name").fill("Nope");
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn", { hasText: "Cancel" }).click();
  await expect(modal(page)).toBeHidden();
  await page.evaluate(() => window.opencalcEditor.runCommand("formulas.define-name"));
  await field(page, "name").fill("Nope");
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeHidden();
  expect(await names(page)).toEqual([]);
});
