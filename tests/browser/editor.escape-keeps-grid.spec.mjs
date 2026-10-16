// Escape returns to the grid, and the grid keeps the keyboard (UX-SHELL-01).
//
// The ribbon's global keydown handler closed the File page on *every* Escape,
// and closing it focused the active ribbon tab whether or not the page had been
// open. So cancelling a cell edit — the most common Escape there is — moved
// focus to "Home", and whatever the person typed next went to the tab strip
// instead of the sheet. Nothing failed loudly: the keystrokes simply vanished.
//
// Every earlier spec pressed Escape and then asserted on the cell. None typed
// after it, which is the only way to see where focus went.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

/// Whether the grid holds the keyboard. Not `activeElement === #grid`: while
/// no edit is open the grid's keys arrive through the idle in-cell editor, so
/// an input method can compose into it (XL-FEAT-023).
const focused = (page) =>
  page.evaluate(() => (window.opencalcEditor.gridHasKeyboard() ? "grid" : `${document.activeElement?.tagName}#${document.activeElement?.id}`));
const cell = (page, row, col) => page.locator(`#a11y-${row}-${col}`);

test("Escape out of a cell edit leaves the grid holding the keyboard", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 5, 2, 5)); // F3

  await page.keyboard.type("discard me");
  await page.keyboard.press("Escape");
  expect(await focused(page), "Escape moved focus off the grid").toBe("grid");

  // The consequence as a person meets it: the next thing typed lands in the sheet.
  await page.keyboard.type("7");
  await page.keyboard.press("Enter");
  await expect(cell(page, 2, 5)).toHaveText("7");
});

test("Escape with nothing open does not move focus", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Escape");
  expect(await focused(page)).toBe("grid");
});

test("Escape still closes the File page and returns to its tab", async ({ page }) => {
  await boot(page);
  await page.locator(".rb-file").click();
  await expect(page.locator(".rb-backstage")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".rb-backstage")).toBeHidden();
});
