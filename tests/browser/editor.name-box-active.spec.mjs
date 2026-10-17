// The name box makes a range's top-left cell the active one (XL-UX-165).
//
// Excel: type `B2:D5` in the name box and press Enter, and B2:D5 is selected
// with B2 active — the name box then reads B2, and what is typed next goes
// into B2. The Go To dialog already did this (`selectBoxes`); the name box
// (`gotoName`) put the active cell at the bottom-right, so typing landed in D5.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_define_name("Block", "Sheet1!$C$3:$D$6");
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}

const nameBox = (page) => page.locator("#cell-ref");
const go = async (page, text) => {
  await nameBox(page).fill(text);
  await nameBox(page).press("Enter");
};
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);

test("typing B2:D5 leaves B2 active, and the next entry lands in B2", async ({ page }) => {
  await boot(page);
  await go(page, "B2:D5");
  expect(await sel(page)).toEqual({ r0: 1, c0: 1, r1: 4, c1: 3 });
  await expect(nameBox(page)).toHaveValue("B2");
  await page.keyboard.type("first");
  await page.keyboard.press("Enter");
  expect(await input(page, 1, 1)).toBe("first");
  expect(await input(page, 4, 3)).toBe("");
});

test("a range typed backwards still activates its top-left", async ({ page }) => {
  await boot(page);
  await go(page, "D5:B2");
  await expect(nameBox(page)).toHaveValue("B2");
});

test("a defined name's range activates its top-left", async ({ page }) => {
  await boot(page);
  await go(page, "Block");
  expect(await sel(page)).toEqual({ r0: 2, c0: 2, r1: 5, c1: 3 });
  await expect(nameBox(page)).toHaveValue("C3");
});

test("a list of ranges activates the first range's top-left", async ({ page }) => {
  await boot(page);
  await go(page, "B2:C3,E5:F6");
  await expect(nameBox(page)).toHaveValue("B2");
});
