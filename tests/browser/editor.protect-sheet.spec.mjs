// Excel's Protect Sheet dialog (fit-gap XL-UX-023).
//
// Review ▸ Protect Sheet and Home ▸ Format ▸ Protect Sheet open a dialog: the
// "Protect worksheet and contents of locked cells" box, an optional password,
// and the "Allow all users of this worksheet to" list with the two Select
// options ticked. A password asks for confirmation; unprotecting a sheet with
// one asks for it. OpenCalc toggled protection instantly, with no dialog, no
// password and no options.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.locator("#grid").focus();
}
const modal = (page) => page.locator("#oc-modal");
const title = (page) => page.locator("#oc-modal-title");
const ok = (page) => page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary");
const protection = (page) => page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_sheet_protection(0)));
const protect = (page) => page.evaluate(() => window.opencalcEditor.runCommand("format.protection.protect-this-sheet"));

const OPTIONS = [
  "Select locked cells", "Select unlocked cells", "Format cells", "Format columns", "Format rows",
  "Insert columns", "Insert rows", "Insert hyperlinks", "Delete columns", "Delete rows", "Sort",
  "Use AutoFilter", "Use PivotTable & PivotChart", "Edit objects", "Edit scenarios",
];

test("Protect Sheet opens Excel's dialog with its defaults", async ({ page }) => {
  await boot(page);
  await protect(page);
  await expect(modal(page)).toBeVisible();
  await expect(title(page)).toHaveText("Protect Sheet");
  await expect(page.locator("#oc-prot-on")).toBeChecked();
  await expect(page.locator("label[for=oc-prot-on], .oc-prot-master")).toContainText("Protect worksheet and contents of locked cells");
  await expect(page.locator("#oc-prot-pw")).toHaveAttribute("type", "password");
  await expect(page.locator(".oc-prot-allow .oc-check span")).toHaveText(OPTIONS);
  const checked = await page.locator(".oc-prot-allow input").evaluateAll((xs) => xs.map((x) => x.checked));
  expect(checked).toEqual(OPTIONS.map((_, i) => i < 2));
  await expect(page.locator("#oc-prot-pw")).toBeFocused();
});

test("OK without a password protects with the options chosen", async ({ page }) => {
  await boot(page);
  await protect(page);
  await page.locator(".oc-prot-allow .oc-check", { hasText: "Format columns" }).locator("input").check();
  await page.locator(".oc-prot-allow .oc-check", { hasText: "Select locked cells" }).locator("input").uncheck();
  await ok(page).click();
  await expect(modal(page)).toBeHidden();
  const p = await protection(page);
  expect(p.on).toBe(true);
  expect(p.password).toBe(false);
  expect(p.allow.formatColumns).toBe(true);
  expect(p.allow.selectLockedCells).toBe(false);
  expect(p.allow.selectUnlockedCells).toBe(true);
  expect(p.allow.formatRows).toBe(false);
});

test("unticking the master box disables OK, as in Excel", async ({ page }) => {
  await boot(page);
  await protect(page);
  await page.locator("#oc-prot-on").uncheck();
  await expect(ok(page)).toBeDisabled();
});

test("a password is confirmed, then demanded to unprotect", async ({ page }) => {
  await boot(page);
  await protect(page);
  await page.locator("#oc-prot-pw").fill("s3cret");
  await page.keyboard.press("Enter");
  await expect(title(page)).toHaveText("Confirm Password");
  await page.locator("#oc-prot-confirm").fill("wrong");
  await page.keyboard.press("Enter");
  await expect(page.locator("#oc-prot-error")).toHaveText("Confirmation password is not identical.");
  expect((await protection(page)).on).toBe(false);
  await page.locator("#oc-prot-confirm").fill("s3cret");
  await page.keyboard.press("Enter");
  await expect(modal(page)).toBeHidden();
  expect(await protection(page)).toMatchObject({ on: true, password: true });

  // The same command now unprotects — and asks for the password.
  await protect(page);
  await expect(title(page)).toHaveText("Unprotect Sheet");
  await page.locator("#oc-prot-unpw").fill("S3CRET");
  await page.keyboard.press("Enter");
  await expect(page.locator("#oc-prot-error")).toContainText("The password you supplied is not correct.");
  expect((await protection(page)).on).toBe(true);
  await page.locator("#oc-prot-unpw").fill("s3cret");
  await ok(page).click();
  await expect(modal(page)).toBeHidden();
  expect((await protection(page)).on).toBe(false);
});

test("a sheet protected without a password unprotects at once", async ({ page }) => {
  await boot(page);
  await protect(page);
  await ok(page).click();
  expect((await protection(page)).on).toBe(true);
  await protect(page);
  await expect(modal(page)).toBeHidden();
  expect((await protection(page)).on).toBe(false);
});

test("Cancel protects nothing", async ({ page }) => {
  await boot(page);
  await protect(page);
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeHidden();
  expect((await protection(page)).on).toBe(false);
});
