// Escape closes the menu that is open (UX-SHELL-09).
//
// The right-click menu, the sheet-tab menu and the Name Manager all open as
// `#sheet-ctx`, and only a click elsewhere removed it. Escape did nothing, so
// the menu stayed on screen across the dialogs opened after it — found by
// opening every dialog in turn and seeing a 260px menu in each screenshot.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

for (const [what, open] of [
  ["the Name Manager", (page) => page.evaluate(() => window.opencalcEditor.runCommand("tools.name-manager"))],
  ["the cell's right-click menu", (page) => page.evaluate(() => window.opencalcEditor.cellMenu(300, 300))],
]) {
  test(`Escape closes ${what} and gives the grid the keyboard back`, async ({ page }) => {
    await boot(page);
    await open(page);
    await expect(page.locator("#sheet-ctx")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("#sheet-ctx")).toHaveCount(0);
    expect(await page.evaluate(() => document.activeElement?.id)).toBe("grid");
  });
}

// The toolbar's own popups (fill and font colour, borders, orientation, merge)
// opened from a ribbon button are re-anchored under it; Escape must close them
// too, or they linger across whatever is done next (fit-gap XL-UX-004).
for (const id of ["toolbar.fillcolor", "toolbar.fontcolor", "toolbar.rotate", "toolbar.merge"]) {
  test(`Escape closes the ${id} popup opened from the ribbon`, async ({ page }) => {
    await boot(page);
    await page.locator(`.oc-ribbon [data-cmd="${id}"]`).click();
    const open = page.locator(".toolbar .popmenu:not([hidden])");
    await expect(open).toHaveCount(1);
    await page.keyboard.press("Escape");
    await expect(open).toHaveCount(0);
  });
}
