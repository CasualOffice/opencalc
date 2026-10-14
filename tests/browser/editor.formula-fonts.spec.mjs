// The formula bar, name box and function hints use the UI font; the in-cell
// editor uses the cell's own font (XL-UX-018).
//
// All of them were a monospace stack (ui-monospace, SFMono, Menlo…), so typing
// into a Calibri cell switched to a code font and the formula bar read as a
// terminal. Excel draws the formula bar and name box in its UI font, and edits
// a cell in the cell's font at the cell's size.

import { expect, test } from "@playwright/test";

const MONO = /mono|menlo|consolas|courier/i;

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

test("the formula bar and name box are in the UI font", async ({ page }) => {
  await boot(page);
  const fonts = await page.evaluate(() => ({
    nameBox: getComputedStyle(document.getElementById("cell-ref")).fontFamily,
    formula: getComputedStyle(document.getElementById("formula-input")).fontFamily,
    body: getComputedStyle(document.body).fontFamily,
  }));
  expect(fonts.nameBox, "name box").not.toMatch(MONO);
  expect(fonts.formula, "formula bar").not.toMatch(MONO);
  expect(fonts.formula).toBe(fonts.body);
});

test("the function list and argument tip are in the UI font", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.locator("#grid").focus();
  await page.keyboard.type("=SUM");
  await expect(page.locator(".ac-name").first()).toBeVisible();
  expect(await page.locator(".ac-name").first().evaluate((e) => getComputedStyle(e).fontFamily)).not.toMatch(MONO);
  await page.keyboard.type("(");
  await expect(page.locator(".sig-tip")).toBeVisible();
  expect(await page.locator(".sig-tip").evaluate((e) => getComputedStyle(e).fontFamily)).not.toMatch(MONO);
});

for (const [fn, pt] of [["Calibri", 11], ["Times New Roman", 16]]) {
  test(`the in-cell editor takes the cell's font: ${fn} ${pt}pt`, async ({ page }) => {
    await boot(page);
    const expected = await page.evaluate(([fn, pt]) => {
      const ed = window.opencalcEditor;
      const api = ed.wasmApi();
      api.session_new();
      api.session_set_cell(0, 1, 1, "text");
      ed.selectForTest(1, 1);
      ed.runCommand && null;
      // Font name and size through the editor's own toolbar fields.
      return { first: api.font_css_stack(fn).split(",")[0].replace(/["']/g, "").trim(), px: Math.round((pt * 4) / 3) };
    }, [fn, pt]);
    await page.locator("#tb-font").fill(fn);
    await page.locator("#tb-font").press("Enter");
    await page.locator("#tb-size").fill(String(pt));
    await page.locator("#tb-size").press("Enter");
    await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
    await page.locator("#grid").focus();
    await page.keyboard.press("F2");
    const ed = page.locator(".inline-edit");
    await expect(ed).toBeVisible();
    const f = await ed.evaluate((e) => ({ family: getComputedStyle(e).fontFamily, size: parseFloat(getComputedStyle(e).fontSize) }));
    expect(f.family, "in-cell editor family").not.toMatch(MONO);
    expect(f.family.replace(/["']/g, "")).toContain(expected.first);
    expect(Math.abs(f.size - expected.px), `size ${f.size} vs ${expected.px}`).toBeLessThan(1);
  });
}

/// Re-editing a cell after its font changed uses the new font, not the one the
/// editor saw the last time it opened there.
test("the in-cell editor follows a font change between edits", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.wasmApi().session_set_cell(0, 1, 1, "text");
    ed.selectForTest(1, 1);
  });
  await page.locator("#grid").focus();
  await page.keyboard.press("F2");
  await page.keyboard.press("Escape");
  await page.locator("#tb-size").fill("20");
  await page.locator("#tb-size").press("Enter");
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.locator("#grid").focus();
  await page.keyboard.press("F2");
  const size = await page.locator(".inline-edit").evaluate((e) => parseFloat(getComputedStyle(e).fontSize));
  expect(Math.abs(size - Math.round((20 * 4) / 3)), `editor at ${size}px after the cell became 20pt`).toBeLessThan(1);
});
