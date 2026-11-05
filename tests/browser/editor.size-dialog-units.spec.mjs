// Column Width and Row Height speak Excel's units (fit-gap XL-UX-035).
//
// Microsoft's "Change the column width and row height": a column is measured
// in characters of the default font (0 to 255, default 8.43), a row in points
// (0 to 409, default 15.00), and 0 hides the line. The dialog's boxes are
// labelled "Column width" and "Row height".
//
// Ours asked for pixels and offered 64 for a fresh column, so a number copied
// from Excel's dialog — or from a colleague's instructions, "set it to 20" —
// produced a column a third as wide as intended. The conversion is the one the
// xlsx reader and writer use (`casual_calc_model::units`), reached through the
// engine, so the dialog cannot disagree with a saved file about a width.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const api = (page, fn, arg) => page.evaluate(fn, arg);
const widthPx = (page, c) => api(page, (c) => window.opencalcEditor.wasmApi().session_col_width(0, c), c);
const heightPx = (page, r) => api(page, (r) => window.opencalcEditor.wasmApi().session_row_height(0, r), r);
const field = (page) => page.locator("#oc-size-value");
const ask = (page, axis, at) => api(page, ([ax, i]) => {
  const e = window.opencalcEditor;
  if (ax === "col") e.selectForTest(0, i); else e.selectForTest(i, 0);
  e.sizeDialog(ax, i);
}, [axis, at]);

test("a default column offers 8.43 characters, and a default row 15 points", async ({ page }) => {
  await boot(page);
  await ask(page, "col", 1);
  await expect(page.locator('label[for="oc-size-value"]')).toHaveText("Column width:");
  await expect(field(page)).toHaveValue("8.43");
  await page.keyboard.press("Escape");
  await ask(page, "row", 1);
  await expect(page.locator('label[for="oc-size-value"]')).toHaveText("Row height:");
  await expect(field(page)).toHaveValue("15");
});

test("typing 8.43 gives back the default 64 px column; 20 characters is 145 px", async ({ page }) => {
  await boot(page);
  await api(page, () => window.opencalcEditor.wasmApi().session_set_col_width(0, 1, 100));
  await ask(page, "col", 1);
  await expect(field(page)).toHaveValue("13.57"); // (100 - 5) / 7, ECMA-376's rounding
  await field(page).fill("8.43");
  await page.keyboard.press("Enter");
  await expect.poll(() => widthPx(page, 1)).toBe(64);

  await ask(page, "col", 2);
  await field(page).fill("20");
  await page.keyboard.press("Enter");
  await expect.poll(() => widthPx(page, 2)).toBe(145);
});

test("a row height is points: 30 pt is 40 px", async ({ page }) => {
  await boot(page);
  await ask(page, "row", 2);
  await field(page).fill("30");
  await page.keyboard.press("Enter");
  await expect.poll(() => heightPx(page, 2)).toBe(40);
});

test("0 hides the column, as it does in Excel", async ({ page }) => {
  await boot(page);
  await ask(page, "col", 3);
  await field(page).fill("0");
  await page.keyboard.press("Enter");
  // A hidden column draws 0 px wide; its own width is kept for Unhide.
  await expect.poll(() => api(page, () => JSON.parse(window.opencalcEditor.wasmApi().session_col_px(0, 3, 1))[0])).toBe(0);
  await api(page, () => window.opencalcEditor.wasmApi().session_unhide_cols(0, 3, 3));
  expect(await widthPx(page, 3), "hiding threw the width away").toBe(64);
});

test("past Excel's limit is refused with the limit stated", async ({ page }) => {
  await boot(page);
  await ask(page, "col", 1);
  await field(page).fill("256");
  await page.keyboard.press("Enter");
  await expect(page.locator("#oc-msgbox-text")).toHaveText("Column width must be between 0 and 255.");
  await page.keyboard.press("Enter");
  await expect(field(page)).toHaveValue("256");
  expect(await widthPx(page, 1)).toBe(64);
});
