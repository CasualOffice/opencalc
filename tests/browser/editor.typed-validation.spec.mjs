// A validation rule judges the value an entry becomes, not the text typed
// (XL-FEAT-127).
//
// Excel stores `50%` as 0.5, `$1,200` as 1200 and `3/15/2026` as a date serial,
// and its data validation checks that stored value — so a decimal rule between
// 0 and 1 is how a rate column is restricted, and `50%` is how anyone types into
// it. OpenCalc checked the raw text against `parseFloat`-style reading, refused
// all three, and the only way past the rule was to type the decimal the percent
// format exists to spare people from typing.

import { expect, test } from "@playwright/test";

async function boot(page, kind, op, f1, f2) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(([kind, op, f1, f2]) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_set_validation(0, 0, 0, 9, 0, kind, op, f1, f2, true, "");
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.draw();
  }, [kind, op, f1, f2]);
  await page.locator("#grid").focus();
}

const box = (page) => page.locator("#oc-msgbox");
const value = (page) => page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 0));
const typeInA1 = async (page, text) => {
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.locator("#grid").focus();
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
};

test("typing 50% into a decimal 0-1 rule is accepted, as the 0.5 it stores", async ({ page }) => {
  await boot(page, "decimal", "between", "0", "1");
  await typeInA1(page, "50%");
  await expect(box(page), "no alert: 0.5 is inside 0..1").toBeHidden();
  expect(await value(page)).toBe("0.5");
});

test("150% is still refused by the same rule", async ({ page }) => {
  await boot(page, "decimal", "between", "0", "1");
  await typeInA1(page, "150%");
  await expect(box(page)).toBeVisible();
  expect(await value(page), "nothing is written while it asks").toBe("");
});

test("currency and thousands separators are judged as the number they store", async ({ page }) => {
  await boot(page, "whole", "between", "1000", "5000");
  await typeInA1(page, "$1,200");
  await expect(box(page)).toBeHidden();
  expect(await value(page)).toBe("1200");
});

test("a date typed into a date rule is judged as a date, against bounds typed as dates", async ({ page }) => {
  await boot(page, "date", "between", "1/1/2026", "12/31/2026");
  await typeInA1(page, "3/15/2026");
  await expect(box(page), "inside the year").toBeHidden();
  await typeInA1(page, "3/15/2027");
  await expect(box(page), "outside it").toBeVisible();
});
