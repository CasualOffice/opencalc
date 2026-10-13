// A new workbook is blank (UX-SHELL-04).
//
// Every untitled workbook opened with someone else's Item/Qty/Price/Total rows
// in it, written as document content and marked saved, so a person's first act
// in the product was deleting a stranger's data. The demo sheet now loads only
// when asked for, with `?sample=demo`.

import { expect, test } from "@playwright/test";

const cell = (page, row, col) => page.locator(`#a11y-${row}-${col}`);

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

test("the editor opens on an empty sheet", async ({ page }) => {
  await boot(page);
  for (const [r, c] of [[0, 0], [0, 3], [4, 3]]) await expect(cell(page, r, c)).toHaveText("");
  await expect(page.locator("#tb-undo")).toBeDisabled();
});

test("the demo sheet is still one parameter away", async ({ page }) => {
  await boot(page, "?sample=demo");
  await expect(cell(page, 0, 0)).toHaveText("Item");
  await expect(cell(page, 4, 3)).toHaveText("43.48");
});

test("the demo sheet does not take its colours from the OS theme", async ({ browser }) => {
  // The header fill was chosen from `prefers-color-scheme` and written into the
  // document, so a file made in dark mode carried a dark header everywhere.
  const fills = [];
  for (const colorScheme of ["light", "dark"]) {
    const page = await (await browser.newContext({ colorScheme })).newPage();
    await boot(page, "?sample=demo");
    fills.push(await page.evaluate(() => JSON.parse(window.opencalcEditor.wasm.session_cell_format(0, 0, 0))));
  }
  expect(fills[0], "the OS theme changed the document").toEqual(fills[1]);
});
