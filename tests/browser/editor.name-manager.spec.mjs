// Excel's Name Manager (fit-gap XL-UX-021).
//
// Ctrl+F3 and Formulas ▸ Name Manager open a dialog listing every defined name
// with its Value, Refers To, Scope and Comment; a Filter; New…, Edit… and
// Delete; and a Refers to box that edits the selected name. OpenCalc opened a
// small popover near the name box that showed the name and its target, with an
// unconfirmed × that deleted it.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    let names = JSON.parse(a.session_sheet_names());
    if (names.length < 2) a.session_add_sheet();
    names = JSON.parse(a.session_sheet_names());
    const q = (n) => (/[^A-Za-z0-9_]/.test(n) ? `'${n}'` : n);
    a.session_set_cell(0, 0, 7, "12");
    a.session_set_cell(0, 1, 7, "30");
    a.session_set_name("", -1, "Rate", -1, `${q(names[0])}!$H$1`, "VAT rate");
    a.session_set_name("", -1, "Pair", 1, `${q(names[0])}!$H$1:$H$2`, "");
    a.session_set_name("", -1, "Broken", -1, "#REF!", "");
    window.__sheet0 = q(names[0]);
    window.__sheet1 = names[1];
  });
  await page.locator("#grid").focus();
}
const modal = (page) => page.locator("#oc-modal");
const rows = (page) => page.locator("#oc-nm-list .oc-nm-row");
const row = (page, name) => rows(page).filter({ has: page.locator(`[data-col="name"]`, { hasText: new RegExp(`^${name}$`) }) });
const names = (page) => page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_names()));

for (const [how, open] of [
  ["Ctrl+F3", (p) => p.keyboard.press("Control+F3")],
  ["Formulas ▸ Name Manager", (p) => p.evaluate(() => window.opencalcEditor.runCommand("tools.name-manager"))],
]) {
  test(`${how} opens the Name Manager dialog`, async ({ page }) => {
    await boot(page);
    await open(page);
    await expect(modal(page)).toBeVisible();
    await expect(page.locator("#oc-modal-title")).toHaveText("Name Manager");
    await expect(page.locator("#oc-nm-list .oc-nm-head [data-col]")).toHaveText(["Name", "Value", "Refers To", "Scope", "Comment"]);
    await expect(page.locator("#oc-nm-new")).toHaveText("New…");
    await expect(page.locator("#oc-nm-edit")).toHaveText("Edit…");
    await expect(page.locator("#oc-nm-delete")).toHaveText("Delete");
    await expect(page.locator("#oc-modal .oc-confirm-actions .oc-btn")).toHaveText(["Close"]);
    await expect(page.locator("#sheet-ctx"), "no popover any more").toHaveCount(0);
  });
}

test("each name shows its value, target, scope and comment", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  const s0 = await page.evaluate(() => window.__sheet0);
  const s1 = await page.evaluate(() => window.__sheet1);
  await expect(rows(page).locator('[data-col="name"]')).toHaveText(["Broken", "Pair", "Rate"]);
  const rate = row(page, "Rate");
  await expect(rate.locator('[data-col="value"]')).toHaveText("12");
  await expect(rate.locator('[data-col="refers"]')).toHaveText(`=${s0}!$H$1`);
  await expect(rate.locator('[data-col="scope"]')).toHaveText("Workbook");
  await expect(rate.locator('[data-col="comment"]')).toHaveText("VAT rate");
  const pair = row(page, "Pair");
  await expect(pair.locator('[data-col="value"]')).toHaveText("{12;30}");
  await expect(pair.locator('[data-col="scope"]')).toHaveText(s1);
  await expect(row(page, "Broken").locator('[data-col="value"]')).toHaveText("#REF!");
});

test("the Filter narrows the list as Excel's does", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  const filter = page.locator("#oc-nm-filter");
  const shown = () => rows(page).locator('[data-col="name"]');
  await filter.selectOption({ label: "Names Scoped to Worksheet" });
  await expect(shown()).toHaveText(["Pair"]);
  await filter.selectOption({ label: "Names Scoped to Workbook" });
  await expect(shown()).toHaveText(["Broken", "Rate"]);
  await filter.selectOption({ label: "Names with Errors" });
  await expect(shown()).toHaveText(["Broken"]);
  await filter.selectOption({ label: "Names without Errors" });
  await expect(shown()).toHaveText(["Pair", "Rate"]);
  await filter.selectOption({ label: "Clear Filter" });
  await expect(shown()).toHaveCount(3);
});

test("the Refers to box edits the selected name", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  const ref = page.locator("#oc-nm-ref");
  await expect(ref).toBeDisabled();
  await row(page, "Rate").click();
  const s0 = await page.evaluate(() => window.__sheet0);
  await expect(ref).toHaveValue(`=${s0}!$H$1`);
  await ref.fill(`=${s0}!$H$2`);
  await ref.press("Enter");
  await expect(modal(page), "Enter commits the box, it does not close the dialog").toBeVisible();
  await expect(row(page, "Rate").locator('[data-col="value"]')).toHaveText("30");
  expect((await names(page)).find((n) => n.name === "Rate").refersTo).toBe(`${s0}!$H$2`);
});

test("Delete asks first, and only deletes on OK", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  await row(page, "Rate").click();
  await page.locator("#oc-nm-delete").click();
  await expect(page.locator("#oc-modal-body")).toContainText("Are you sure you want to delete the name Rate?");
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn", { hasText: "Cancel" }).click();
  await expect(page.locator("#oc-modal-title"), "Cancel goes back to the Name Manager").toHaveText("Name Manager");
  expect((await names(page)).map((n) => n.name)).toContain("Rate");

  await row(page, "Rate").click();
  await page.locator("#oc-nm-delete").click();
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Name Manager");
  await expect(rows(page).locator('[data-col="name"]')).toHaveText(["Broken", "Pair"]);
  expect((await names(page)).map((n) => n.name)).not.toContain("Rate");
});

test("Escape closes it and gives the grid the keyboard back", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  await expect(modal(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeHidden();
  expect(await page.evaluate(() => window.opencalcEditor.gridHasKeyboard())).toBe(true);
});
