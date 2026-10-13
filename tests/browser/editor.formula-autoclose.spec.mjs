// A formula missing its closing brackets is completed, not refused (UX-SHELL-02).
//
// Excel appends the `)` a formula is missing and commits it. The editor refused
// the entry, kept the cell in edit mode and reported "the formula looks
// incomplete" in the status bar — for the single most common slip in formula
// entry. Only brackets that are provably missing are added: an unterminated
// string is closed first, and brackets inside a string or a quoted sheet name
// are not counted.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const cell = (page, row, col) => page.locator(`#a11y-${row}-${col}`);

async function enterAt(page, row, col, text) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c, r, c), [row, col]);
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
}

async function formulaAt(page, row, col) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c, r, c), [row, col]);
  return page.locator("#formula-input").inputValue();
}

test("a missing closing bracket is added and the formula commits", async ({ page }) => {
  await boot(page);
  await enterAt(page, 2, 5, "=SUM(B2:B4");
  await expect(cell(page, 2, 5)).toHaveText("10");
  expect(await formulaAt(page, 2, 5)).toBe("=SUM(B2:B4)");
});

test("nested brackets are all closed", async ({ page }) => {
  await boot(page);
  await enterAt(page, 2, 5, "=ROUND(SUM(B2:B4)/3,1");
  await expect(cell(page, 2, 5)).toHaveText("3.3");
});

test("a bracket inside a string is not counted", async ({ page }) => {
  await boot(page);
  await enterAt(page, 2, 5, '=LEN("(")');
  await expect(cell(page, 2, 5)).toHaveText("1");
  expect(await formulaAt(page, 2, 5)).toBe('=LEN("(")');
});

test("an unterminated string is closed before the brackets", async ({ page }) => {
  await boot(page);
  await enterAt(page, 2, 5, '=LEN("abc');
  await expect(cell(page, 2, 5)).toHaveText("3");
});
