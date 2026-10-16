// Excel's Go To dialog (fit-gap XL-UX-019).
//
// Ctrl+G, F5 and Home ▸ Find & Select ▸ Go To open a dialog: the workbook's
// named ranges and the references recently gone to, a Reference box, OK and
// Cancel. OpenCalc's `edit.go-to` only focused the name box, so none of the
// names were listed and nothing remembered where you had been.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    const names = JSON.parse(a.session_sheet_names());
    const q = /[^A-Za-z0-9_]/.test(names[0]) ? `'${names[0]}'` : names[0];
    a.session_define_name("Rates", `${q}!$C$3:$D$6`);
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());
const modal = (page) => page.locator("#oc-modal");
const reference = (page) => page.locator("#oc-goto-ref");

for (const [how, open] of [
  ["Ctrl+G", (p) => p.keyboard.press("Control+g")],
  ["F5", (p) => p.keyboard.press("F5")],
  ["Find & Select ▸ Go To", (p) => p.evaluate(() => window.opencalcEditor.runCommand("edit.go-to"))],
]) {
  test(`${how} opens the Go To dialog`, async ({ page }) => {
    await boot(page);
    await open(page);
    await expect(modal(page)).toBeVisible();
    await expect(page.locator("#oc-modal-title")).toHaveText("Go To");
    await expect(page.locator("#oc-goto-list")).toContainText("Rates");
    await expect(reference(page)).toBeFocused();
  });
}

test("a typed cell reference goes there", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await reference(page).fill("c5");
  await page.keyboard.press("Enter");
  await expect(modal(page)).toBeHidden();
  expect(await sel(page)).toEqual({ r0: 4, c0: 2, r1: 4, c1: 2 });
  await expect(page.locator("#cell-ref")).toHaveValue("C5");
  expect(await page.evaluate(() => document.activeElement?.id)).toBe("grid");
});

test("a range, an absolute reference and a defined name are all accepted", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await reference(page).fill("B2:D4");
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await sel(page)).toEqual({ r0: 1, c0: 1, r1: 3, c1: 3 });

  await page.keyboard.press("Control+g");
  await reference(page).fill("=$E$7");
  await page.keyboard.press("Enter");
  expect(await sel(page)).toEqual({ r0: 6, c0: 4, r1: 6, c1: 4 });

  // Picking a name from the list fills the box; OK goes to its range.
  await page.keyboard.press("Control+g");
  await page.locator("#oc-goto-list [role=option]", { hasText: "Rates" }).click();
  await expect(reference(page)).toHaveValue("Rates");
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await sel(page)).toEqual({ r0: 2, c0: 2, r1: 5, c1: 3 });
});

test("a sheet-qualified reference switches sheet", async ({ page }) => {
  await boot(page);
  const second = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    const i = a.session_add_sheet();
    return JSON.parse(a.session_sheet_names())[i];
  });
  await page.keyboard.press("Control+g");
  await reference(page).fill(`'${second}'!B3`);
  await page.keyboard.press("Enter");
  await expect(modal(page)).toBeHidden();
  await expect(page.locator(".sheet-tab.active")).toContainText(second);
  expect(await sel(page)).toEqual({ r0: 2, c0: 1, r1: 2, c1: 1 });
});

test("an invalid reference says so and keeps the dialog open", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  for (const bad of ["1abc", "NoSuchName", "Nowhere!A1"]) {
    await reference(page).fill(bad);
    await page.keyboard.press("Enter");
    await expect(modal(page), `${bad} closed the dialog`).toBeVisible();
    // Excel's message box over the dialog (XL-UX-168); OK returns to the box.
    await expect(page.locator("#oc-msgbox-text")).toHaveText("Reference isn't valid.");
    await page.keyboard.press("Enter");
    await expect(page.locator("#oc-msgbox")).toBeHidden();
    await expect(reference(page)).toBeFocused();
  }
  expect(await sel(page), "an invalid reference moved the selection").toEqual({ r0: 0, c0: 0, r1: 0, c1: 0 });
  // A refused Go To defines nothing either — the name box's habit of turning an
  // unknown word into a new name has no business here.
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_names())).not.toContain("NoSuchName");
});

test("references gone to are remembered at the top of the list", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await reference(page).fill("C5");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Control+g");
  const first = page.locator("#oc-goto-list [role=option]").first();
  await expect(first).toHaveText(/\$C\$5$/);
  // Picking it fills the box with it, as Excel does.
  await first.click();
  await expect(reference(page)).toHaveValue(/\$C\$5$/);
});

test("Escape and Cancel leave the selection alone", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await reference(page).fill("H9");
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeHidden();
  await page.keyboard.press("Control+g");
  await reference(page).fill("H9");
  await page.locator("#oc-modal .oc-btn", { hasText: "Cancel" }).click();
  await expect(modal(page)).toBeHidden();
  expect(await sel(page)).toEqual({ r0: 0, c0: 0, r1: 0, c1: 0 });
});
