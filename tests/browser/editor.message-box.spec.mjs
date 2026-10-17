// Excel's message box over a dialog (XL-UX-168, docs/95 rule 8).
//
// Excel answers a dialog's invalid input with a message box raised over the
// dialog: a title bar ("Microsoft Excel"; "OpenCalc" here, our own name), an
// icon, the message and OK. OK has the focus; Enter or Escape dismisses it;
// the dialog underneath keeps what was typed and the field the message is
// about gets the focus back with its text selected, ready to retype.
//
// OpenCalc answered all of these inline, in a red line inside the dialog
// (the 2026-10-14 decision XL-UX-147, replaced by the owner on 2026-10-17:
// "assume it as clone of excel"). One shared component now carries every
// message; this spec drives each site through it and asserts the contract
// once per site, so a site left inline — or a second, private message box —
// fails here.

import { expect, test } from "@playwright/test";

async function boot(page, { sample = false } = {}) {
  await page.goto(sample ? "/editor.html?sample=demo" : "/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "apple");   // A1
    a.session_set_cell(0, 1, 0, "pear");    // A2
    a.session_set_cell(0, 2, 0, "apple");   // A3
    a.session_set_cell(0, 0, 1, "1");       // B1
    a.session_set_cell(0, 1, 1, "=B1*2");   // B2
    window.open = () => null;
    ed.selectForTest(0, 0);
    ed.draw();
  });
  await page.locator("#grid").focus();
}

const box = (page) => page.locator("#oc-msgbox");
const dialog = (page) => page.locator("#oc-modal");
const dialogTitle = (page) => page.locator("#oc-modal-title");
const primary = (page) => page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary").first();
const run = (page, id) => page.evaluate((id) => window.opencalcEditor.runCommand(id), id);

/// Every inline message element the dialogs used to show. None may be visible
/// when a message is up: the message is the box, not a line in the dialog.
const INLINE = ".oc-form-error, .panel-error, .oc-fr-msg";

/// Every site, as `[name, trigger, expected]`. `trigger` opens the dialog and
/// submits the bad input; `expected.field` is the input the message is about.
const SITES = [
  ["Go To, an invalid reference", async (page) => {
    await page.keyboard.press("Control+g");
    await page.locator("#oc-goto-ref").fill("1abc");
    await page.keyboard.press("Enter");
  }, { text: "Reference isn't valid.", icon: "warning", title: "Go To", field: "#oc-goto-ref", value: "1abc" }],

  ["Go To Special, nothing found", async (page) => {
    await run(page, "edit.go-to-special");
    await page.locator("#oc-modal .oc-radio", { hasText: /^Notes$/ }).locator("input").check();
    await primary(page).click();
  }, { text: "No cells were found.", icon: "warning", title: "Go To Special", field: null }],

  ["Insert Hyperlink, an invalid place", async (page) => {
    await page.keyboard.press("Control+k");
    await page.locator("#oc-link-to-place").click();
    await page.locator("#oc-link-ref").fill("not a ref");
    await primary(page).click();
  }, { text: "Reference isn't valid.", icon: "warning", title: "Insert Hyperlink", field: "#oc-link-ref", value: "not a ref" }],

  ["Insert Hyperlink, an address that cannot be opened", async (page) => {
    await page.keyboard.press("Control+k");
    await page.locator("#oc-link-address").fill("javascript:alert(1)");
    await primary(page).click();
  }, {
    text: "Only web (http, https) and e-mail addresses can be linked.",
    icon: "warning", title: "Insert Hyperlink", field: "#oc-link-address", value: "javascript:alert(1)",
  }],

  ["Create Table, a range that is not one", async (page) => {
    await page.keyboard.press("Control+t");
    await page.locator("#oc-table-range").fill("nowhere");
    await page.keyboard.press("Enter");
  }, { text: "Reference isn't valid.", icon: "warning", title: "Create table", field: "#oc-table-range", value: "nowhere" }],

  ["Column Width, not a number", async (page) => {
    await page.evaluate(() => window.opencalcEditor.sizeDialog("col", 1));
    await page.locator("#oc-size-px").fill("wide");
    await primary(page).click();
  }, {
    text: "\"wide\" is not a number of pixels — type something like 120.",
    icon: "warning", title: "Column Width", field: "#oc-size-px", value: "wide",
  }],

  ["New Name, an invalid name", async (page) => {
    await run(page, "formulas.define-name");
    await page.locator("#oc-name-name").fill("my name");
    await page.keyboard.press("Enter");
  }, { text: /^The name that you entered is not valid\./, icon: "warning", title: "New Name", field: "#oc-name-name", value: "my name" }],

  ["Confirm Password, a mismatch", async (page) => {
    await run(page, "format.protection.protect-this-sheet");
    await page.locator("#oc-prot-pw").fill("s3cret");
    await page.keyboard.press("Enter");
    await page.locator("#oc-prot-confirm").fill("wrong");
    await page.keyboard.press("Enter");
  }, { text: "The confirmation password is not identical.", icon: "warning", title: "Confirm Password", field: "#oc-prot-confirm", value: "" }],

  ["Unprotect Sheet, a wrong password", async (page) => {
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_protect_sheet(0, "s3cret", ""));
    await run(page, "format.protection.protect-this-sheet");
    await page.locator("#oc-prot-unpw").fill("S3CRET");
    await page.keyboard.press("Enter");
  }, { text: /^The password you supplied is not correct\./, icon: "warning", title: "Unprotect Sheet", field: "#oc-prot-unpw", value: "S3CRET" }],

  ["Goal Seek, a To value that is not a number", async (page) => {
    await run(page, "data.goal-seek");
    await page.locator("#oc-gs-set").fill("B2");
    await page.locator("#oc-gs-to").fill("two");
    await page.locator("#oc-gs-change").fill("B1");
    await primary(page).click();
  }, { text: "Invalid value.", icon: "warning", title: "Goal Seek", field: "#oc-gs-to", value: "two" }],

  ["Goal Seek, a Set cell holding a constant", async (page) => {
    await run(page, "data.goal-seek");
    await page.locator("#oc-gs-set").fill("B1");
    await page.locator("#oc-gs-to").fill("4");
    await page.locator("#oc-gs-change").fill("B1");
    await primary(page).click();
  }, { text: "Cell must contain a formula.", icon: "warning", title: "Goal Seek", field: "#oc-gs-set", value: "B1" }],

  ["Find, nothing found", async (page) => {
    await page.keyboard.press("Control+f");
    await page.locator("#oc-fr-what").fill("durian");
    await page.locator("#oc-modal-body").getByRole("button", { name: "Find Next", exact: true }).click();
  }, {
    text: "We couldn't find what you were looking for. Click Options for more ways to search.",
    icon: "warning", title: "Find and Replace", field: "#oc-fr-what", value: "durian",
  }],

  ["Replace All, the count", async (page) => {
    await page.keyboard.press("Control+h");
    await page.locator("#oc-fr-what").fill("apple");
    await page.locator("#oc-fr-with").fill("plum");
    await page.locator("#oc-modal-body").getByRole("button", { name: "Replace All", exact: true }).click();
  }, { text: "All done. We made 2 replacements.", icon: "info", title: "Find and Replace", field: "#oc-fr-what", value: "apple" }],
];

for (const [name, trigger, want] of SITES) {
  for (const key of ["Enter", "Escape"]) {
    test(`${name}: a message box over the dialog, dismissed by ${key}`, async ({ page }) => {
      await boot(page);
      await trigger(page);

      // The box: our name in its title, the message, Excel's icon, OK focused.
      await expect(box(page)).toBeVisible();
      await expect(box(page).locator("[role=alertdialog]")).toHaveCount(1);
      await expect(page.locator("#oc-msgbox-title")).toHaveText("OpenCalc");
      await expect(page.locator("#oc-msgbox-text")).toHaveText(want.text);
      await expect(box(page)).toHaveAttribute("data-icon", want.icon);
      await expect(page.locator("#oc-msgbox-ok")).toBeFocused();
      // Over the dialog, which is still there with what was typed.
      await expect(dialog(page)).toBeVisible();
      await expect(dialogTitle(page)).toHaveText(want.title);
      if (want.field) await expect(page.locator(want.field)).toHaveValue(want.value);
      const z = await page.evaluate(() => ({
        box: Number(getComputedStyle(document.querySelector("#oc-msgbox")).zIndex),
        dialog: Number(getComputedStyle(document.querySelector("#oc-modal")).zIndex),
      }));
      expect(z.box, "the message box is not above the dialog").toBeGreaterThan(z.dialog);
      // Not inline: no message line inside the dialog.
      expect(await page.locator(INLINE).filter({ visible: true }).count(), "a message was also shown inline").toBe(0);

      // Modal: a key typed while it is up reaches neither the dialog nor the grid.
      await page.keyboard.press("x");
      if (want.field) await expect(page.locator(want.field)).toHaveValue(want.value);

      await page.keyboard.press(key);
      await expect(box(page)).toBeHidden();
      // The dialog survives the dismissal — Escape closes the box, not the dialog.
      await expect(dialog(page)).toBeVisible();
      await expect(dialogTitle(page)).toHaveText(want.title);
      if (want.field) {
        const f = page.locator(want.field);
        await expect(f).toBeFocused();
        await expect(f).toHaveValue(want.value);
        const selected = await f.evaluate((i) => [i.selectionStart, i.selectionEnd, i.value.length]);
        expect(selected, "the field's text is not selected for retyping").toEqual([0, want.value.length, want.value.length]);
      } else {
        const inDialog = await page.evaluate(() => document.querySelector("#oc-modal").contains(document.activeElement));
        expect(inDialog, "focus did not return into the dialog").toBe(true);
      }
    });
  }
}

test("OK by mouse dismisses the box and the dialog can be used again", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await page.locator("#oc-goto-ref").fill("1abc");
  await page.keyboard.press("Enter");
  await page.locator("#oc-msgbox-ok").click();
  await expect(box(page)).toBeHidden();
  await page.locator("#oc-goto-ref").fill("C3");
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeHidden();
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 2, c0: 2, r1: 2, c1: 2 });
});

test("a click outside the box does not dismiss it or reach the dialog", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await page.locator("#oc-goto-ref").fill("1abc");
  await page.keyboard.press("Enter");
  await expect(box(page)).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(box(page)).toBeVisible();
  await expect(dialog(page)).toBeVisible();
});

test("a link to a place that does not exist raises the box over the grid", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_set_cell(0, 1, 2, "go");
    ed.wasmApi().session_set_hyperlink(0, 1, 2, "", "Nowhere!C7", "", "");
    ed.followHyperlink(1, 2);
  });
  await expect(box(page)).toBeVisible();
  await expect(page.locator("#oc-msgbox-text")).toHaveText("Reference isn't valid.");
  await expect(page.locator("#oc-msgbox-ok")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(box(page)).toBeHidden();
  // The grid has the keyboard again — through its input-method host since
  // XL-FEAT-023, so "the active element is #grid" is no longer the question.
  expect(await page.evaluate(() => window.opencalcEditor.gridHasKeyboard())).toBe(true);
});

test("a Name Manager Refers to that does not parse raises the box and keeps the text", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_define_name("Rate", "Sheet1!$B$1"));
  await page.keyboard.press("Control+F3");
  await expect(dialogTitle(page)).toHaveText("Name Manager");
  await page.locator("#oc-modal .oc-nm-row", { hasText: "Rate" }).first().click();
  const ref = page.locator("#oc-nm-ref");
  await ref.fill("=SUM(");
  await ref.press("Enter");
  await expect(box(page)).toBeVisible();
  expect(await page.locator(INLINE).filter({ visible: true }).count()).toBe(0);
  await page.keyboard.press("Enter");
  await expect(box(page)).toBeHidden();
  await expect(dialogTitle(page)).toHaveText("Name Manager");
  await expect(ref).toBeFocused();
  await expect(ref).toHaveValue("=SUM(");
});

test("a conditional-format formula that does not parse raises the box over the panel", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.extendSelectionForTest(2, 1);
  });
  await page.evaluate(() => window.opencalcEditor.runCommand("format.conditional-formatting"));
  await page.locator("#side-panel-body select.panel-select").first().selectOption("formula");
  const formula = page.locator("#side-panel-body .cf-formula");
  await formula.fill("=$A1>");
  await page.locator("#side-panel-body button", { hasText: /^Apply$/ }).click();
  await expect(box(page)).toBeVisible();
  await expect(page.locator("#oc-msgbox-text")).not.toHaveText("");
  expect(await page.locator(INLINE).filter({ visible: true }).count(), "the refusal was also shown inline").toBe(0);
  await page.keyboard.press("Escape");
  await expect(box(page)).toBeHidden();
  await expect(page.locator("#side-panel-body")).toBeVisible();
  await expect(formula).toBeFocused();
  await expect(formula).toHaveValue("=$A1>");
  expect(await formula.evaluate((i) => [i.selectionStart, i.selectionEnd])).toEqual([0, 5]);
});
