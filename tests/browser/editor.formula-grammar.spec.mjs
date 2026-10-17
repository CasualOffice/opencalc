// The formula grammar Excel has and the editor lacked (XL-FEAT-014..017),
// typed key by key into a blank sheet the way a person types it — through the
// formula bar's autocomplete, which must not choke on `{`, `;`, `#` or `@` —
// and checked against the answer Excel gives.
//
// Each is also proven below the browser (the parser, evaluator, xlsx and ODS
// tests and the LibreOffice oracle). What only this file proves is that the
// editor accepts the keystrokes and shows Excel's value and Excel's text.

import { expect, test } from "@playwright/test";

const cell = (page, row, col) => page.locator(`#a11y-${row}-${col}`);

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

async function enterAt(page, row, col, text) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c, r, c), [row, col]);
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
}

async function formulaAt(page, row, col) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c, r, c), [row, col]);
  return page.locator("#formula-input").inputValue();
}

test("a whole-row reference: =SUM(2:2) adds the row (XL-FEAT-016)", async ({ page }) => {
  await boot(page);
  await enterAt(page, 1, 0, "1");
  await enterAt(page, 1, 1, "2");
  await enterAt(page, 1, 2, "3");
  await enterAt(page, 4, 0, "=SUM(2:2)");
  await expect(cell(page, 4, 0)).toHaveText("6");
  expect(await formulaAt(page, 4, 0)).toBe("=SUM(2:2)");
});

test("an array constant: =SUM({1,2;3,4}) is 10, and ={1,2,3} spills (XL-FEAT-014)", async ({ page }) => {
  await boot(page);
  await enterAt(page, 0, 0, "=SUM({1,2;3,4})");
  await expect(cell(page, 0, 0)).toHaveText("10");
  expect(await formulaAt(page, 0, 0)).toBe("=SUM({1,2;3,4})");

  await enterAt(page, 2, 0, '=INDEX({"a","b";"c","d"},2,1)');
  await expect(cell(page, 2, 0)).toHaveText("c");

  await enterAt(page, 4, 0, "={1,2,3}");
  await expect(cell(page, 4, 2)).toHaveText("3");
});

test("the spill operator and implicit intersection: =SUM(A1#), =@A1:A3 (XL-FEAT-015)", async ({ page }) => {
  await boot(page);
  await enterAt(page, 0, 0, "=SEQUENCE(3)");
  await expect(cell(page, 2, 0)).toHaveText("3");
  await enterAt(page, 0, 2, "=SUM(A1#)");
  await expect(cell(page, 0, 2)).toHaveText("6");
  expect(await formulaAt(page, 0, 2)).toBe("=SUM(A1#)");
  // On row 2, `@` picks A2 out of A1:A3, as Excel does.
  await enterAt(page, 1, 3, "=@A1:A3*10");
  await expect(cell(page, 1, 3)).toHaveText("20");
  expect(await formulaAt(page, 1, 3)).toBe("=@A1:A3*10");
  // An error literal still types as one.
  await enterAt(page, 3, 3, "=ISNA(#N/A)");
  await expect(cell(page, 3, 3)).toHaveText("TRUE");
});

test("a 3-D reference sums the same cell across sheets (XL-FEAT-017)", async ({ page }) => {
  await boot(page);
  const [first, last] = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    const i = a.session_add_sheet();
    const j = a.session_add_sheet();
    a.session_set_cell(i, 1, 1, "5");
    a.session_set_cell(j, 1, 1, "7");
    const names = JSON.parse(a.session_sheet_names());
    return [names[i], names[j]];
  });
  await enterAt(page, 0, 0, `=SUM(${first}:${last}!B2)`);
  await expect(cell(page, 0, 0)).toHaveText("12");
  expect(await formulaAt(page, 0, 0)).toBe(`=SUM(${first}:${last}!B2)`);
  // Outside Excel's list of functions that take one, it is #VALUE!.
  await enterAt(page, 1, 0, `=${first}:${last}!B2`);
  await expect(cell(page, 1, 0)).toHaveText("#VALUE!");
});
