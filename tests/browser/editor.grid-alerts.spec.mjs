// Grid and command alerts are Excel's message boxes (XL-UX-174).
//
// Excel answers each of these with a message box, not a line of text:
//
// - Data validation (support.microsoft.com, "More on data validation"): a
//   **Stop** alert "has two options: Retry or Cancel"; with a **Warning**
//   "users can click Yes to accept the invalid entry, No to edit the invalid
//   entry, or Cancel to remove the invalid entry"; with an **Information**
//   alert "users can click OK to accept the invalid value or Cancel to reject
//   it". The rule's own title and message when it has them; otherwise the
//   title is the application's name and the message Excel's default.
// - Typing into a locked cell of a protected sheet, deleting the only sheet,
//   and typing into a PivotTable report.
// - Name Manager and the Macros window ask "Delete…?" over themselves; they
//   used to close, ask, and reopen.
//
// OpenCalc put the first four in the status bar, and let a Warning or an
// Information value through without asking.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "3");   // A1, under a whole-number 1..10 rule
    a.session_set_validation(0, 0, 0, 0, 0, "whole", "between", "1", "10", true, "");
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.draw();
  });
  await page.locator("#grid").focus();
}

const box = (page) => page.locator("#oc-msgbox");
const boxTitle = (page) => page.locator("#oc-msgbox-title");
const boxText = (page) => page.locator("#oc-msgbox-text");
const buttons = (page) => page.locator("#oc-msgbox .oc-msgbox-actions .oc-btn");
const button = (page, label) => page.locator("#oc-msgbox .oc-msgbox-actions .oc-btn", { hasText: new RegExp(`^${label}$`) });
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const style = (page, s, title = "", text = "") => page.evaluate(([s, title, text]) => {
  window.opencalcEditor.wasmApi().session_set_validation_messages(0, 0, 0, 0, 0, s, [title, text, "", ""], false);
}, [s, title, text]);
const typeInA1 = async (page, text) => {
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.locator("#grid").focus();
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
};

const DV_DEFAULT = "This value doesn't match the data validation restrictions defined for this cell.";

test("a Stop rule asks Retry or Cancel, in Excel's default words", async ({ page }) => {
  await boot(page);
  await typeInA1(page, "50");
  await expect(box(page)).toBeVisible();
  await expect(boxTitle(page)).toHaveText("OpenCalc");
  await expect(boxText(page)).toHaveText(DV_DEFAULT);
  await expect(box(page)).toHaveAttribute("data-icon", "error");
  await expect(buttons(page)).toHaveText(["Retry", "Cancel"]);
  await expect(button(page, "Retry")).toBeFocused();
  expect(await input(page, 0, 0), "nothing is written while it asks").toBe("3");
  await expect(page.locator("#tb-status .err")).toHaveCount(0);

  // Retry goes back to the entry with what was typed selected, so typing replaces it.
  await button(page, "Retry").click();
  await expect(box(page)).toBeHidden();
  await page.keyboard.type("7");
  await page.keyboard.press("Enter");
  expect(await input(page, 0, 0)).toBe("7");
});

test("Stop ▸ Cancel puts the old value back", async ({ page }) => {
  await boot(page);
  await typeInA1(page, "50");
  await button(page, "Cancel").click();
  await expect(box(page)).toBeHidden();
  expect(await input(page, 0, 0)).toBe("3");
  // The edit is over: the next key types a new entry rather than amending "50".
  await page.keyboard.type("4");
  await page.keyboard.press("Enter");
  expect(await input(page, 0, 0)).toBe("4");
});

test("a rule's own title and message are what the box says", async ({ page }) => {
  await boot(page);
  await style(page, "stop", "Quantity", "Between 1 and 10, please.");
  await typeInA1(page, "50");
  await expect(boxTitle(page)).toHaveText("Quantity");
  await expect(boxText(page)).toHaveText("Between 1 and 10, please.");
});

test("a Warning asks Continue? with Yes, No and Cancel", async ({ page }) => {
  await boot(page);
  await style(page, "warning");
  await typeInA1(page, "50");
  await expect(box(page)).toHaveAttribute("data-icon", "warning");
  await expect(boxText(page)).toHaveText(`${DV_DEFAULT}\n\nContinue?`);
  await expect(buttons(page)).toHaveText(["Yes", "No", "Cancel"]);
  await button(page, "Yes").click();
  await expect(box(page)).toBeHidden();
  expect(await input(page, 0, 0), "Yes keeps the entry").toBe("50");
  await expect(page.locator("#cell-ref"), "and Enter still moves down").toHaveValue("A2");

  await typeInA1(page, "60");
  await button(page, "No").click();
  await page.keyboard.type("8");
  await page.keyboard.press("Enter");
  expect(await input(page, 0, 0), "No goes back to the entry").toBe("8");

  await typeInA1(page, "70");
  await button(page, "Cancel").click();
  expect(await input(page, 0, 0), "Cancel removes the entry").toBe("8");
});

test("an Information rule offers OK or Cancel", async ({ page }) => {
  await boot(page);
  await style(page, "information");
  await typeInA1(page, "50");
  await expect(box(page)).toHaveAttribute("data-icon", "info");
  await expect(boxText(page)).toHaveText(DV_DEFAULT);
  await expect(buttons(page)).toHaveText(["OK", "Cancel"]);
  await button(page, "Cancel").click();
  expect(await input(page, 0, 0)).toBe("3");
  await typeInA1(page, "50");
  await button(page, "OK").click();
  expect(await input(page, 0, 0)).toBe("50");
});

const PROTECTED =
  "The cell or chart you're trying to change is on a protected sheet. " +
  "To make a change, unprotect the sheet. You might be requested to enter a password.";

test("typing into a locked cell of a protected sheet raises Excel's box", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, true));
  await page.keyboard.type("x");
  await expect(box(page)).toBeVisible();
  await expect(boxText(page)).toHaveText(PROTECTED);
  await expect(buttons(page)).toHaveText(["OK"]);
  await expect(page.locator("#tb-status .err")).toHaveCount(0);
  await page.keyboard.press("Enter");
  await expect(box(page)).toBeHidden();
  expect(await input(page, 0, 0)).toBe("3");
});

test("Delete on a protected sheet raises the same box", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, true));
  await page.keyboard.press("Delete");
  await expect(boxText(page)).toHaveText(PROTECTED);
  expect(await input(page, 0, 0)).toBe("3");
});

test("deleting the only sheet raises Excel's box", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("toolbar.delete-sheet"));
  await expect(box(page)).toBeVisible();
  await expect(boxText(page)).toHaveText(
    "A workbook must contain at least one visible worksheet. To hide, delete, or move the " +
    "selected sheet(s), you must first insert a new sheet or unhide a sheet that is already hidden.",
  );
  await expect(page.locator("#oc-modal")).toBeHidden();
});

test("typing into a PivotTable report raises Excel's box", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_clear_validation(0, 0, 0, 0, 0);
    [["k", "v"], ["x", "1"], ["y", "2"]].forEach((row, r) => row.forEach((v, c) => a.session_set_cell(0, r, c, v)));
    const i = a.session_create_pivot(0, 0, 0, 2, 1, 0, 0, 4, "PT");
    const p = JSON.parse(a.session_pivots(0))[i];
    p.rows = [{ field: 0 }];
    p.values = [{ field: 1, aggregate: "sum" }];
    a.session_set_pivot(0, i, JSON.stringify(p));
    window.opencalcEditor.selectForTest(1, 4);
    window.opencalcEditor.draw();
  });
  await page.locator("#grid").focus();
  await page.keyboard.type("z");
  await expect(box(page)).toBeVisible();
  await expect(boxText(page)).toHaveText("Cannot change this part of a PivotTable report.");
});

test("Name Manager asks Delete? over itself, and stays open", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_define_name("Rate", "Sheet1!$A$1"));
  await page.keyboard.press("Control+F3");
  await page.locator("#oc-nm-list .oc-nm-row", { hasText: "Rate" }).click();
  await page.locator("#oc-nm-delete").click();
  await expect(box(page)).toBeVisible();
  await expect(boxText(page)).toHaveText("Are you sure you want to delete the name Rate?");
  await expect(buttons(page)).toHaveText(["OK", "Cancel"]);
  await expect(page.locator("#oc-modal-title"), "the Name Manager is still behind it").toHaveText("Name Manager");
  await expect(page.locator("#oc-modal")).toBeVisible();
  await button(page, "Cancel").click();
  await expect(page.locator("#oc-nm-list .oc-nm-row", { hasText: "Rate" })).toHaveCount(1);
  await page.locator("#oc-nm-delete").click();
  await button(page, "OK").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Name Manager");
  await expect(page.locator("#oc-nm-list .oc-nm-row", { hasText: "Rate" })).toHaveCount(0);
});

test("the Macros window asks Delete? over itself, and stays open", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await page.locator("#oc-mw-list .oc-mw-row").first().click();
  await page.locator("#oc-mw-delete").click();
  await expect(box(page)).toBeVisible();
  await expect(boxText(page)).toHaveText(/Delete the macro "Macro 1"\?/);
  await expect(buttons(page)).toHaveText(["Yes", "No"]);
  await expect(page.locator("#oc-modal-title")).toHaveText("Macros");
  await button(page, "No").click();
  await expect(page.locator("#oc-mw-list .oc-mw-row")).toHaveCount(1);
  await page.locator("#oc-mw-delete").click();
  await button(page, "Yes").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Macros");
  await expect(page.locator("#oc-mw-list .oc-mw-row")).toHaveCount(0);
});
