// Alt+Down's pick list and a validation input message sit under their cell at
// any zoom (XL-UX-148).
//
// It was placed from the grid's unzoomed geometry, so at 150% it opened a
// third of the way up and to the left of the cell it was for — the validation
// dropdown beside it already multiplied by the zoom.

import { expect, test } from "@playwright/test";

for (const zoom of [100, 150, 200]) {
  test(`the pick list opens under its cell at ${zoom}%`, async ({ page }) => {
    await page.goto("/editor.html?sample=demo");
    await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
    await page.evaluate((z) => {
      const ed = window.opencalcEditor;
      const api = ed.wasmApi();
      api.session_new();
      for (const [r, v] of [[2, "apple"], [3, "pear"], [4, "plum"]]) api.session_set_cell(0, r, 2, v);
      ed.runCommand(`view.zoom.${z}`);
      ed.selectForTest(5, 2);   // C6, under the run
    }, zoom);
    await page.waitForTimeout(150);
    await page.locator("#grid").focus();
    await page.keyboard.press("Alt+ArrowDown");
    const menu = page.locator("#sheet-ctx.dv-menu");
    await expect(menu).toBeVisible();

    const at = await page.evaluate(() => {
      const ed = window.opencalcEditor;
      const b = ed.cellBoxForTest(5, 2);
      const g = document.getElementById("grid").getBoundingClientRect();
      const z = g.width / parseFloat(getComputedStyle(document.getElementById("grid")).width) || 1;
      const zoom = ed.scrollStateForTest?.().zoom ?? z;
      const m = document.getElementById("sheet-ctx").getBoundingClientRect();
      return { zoom, cellLeft: g.left + b.x * zoom, cellBottom: g.top + (b.y + b.h) * zoom, menuLeft: m.left, menuTop: m.top };
    });
    expect(at.zoom, "the zoom did not apply").toBeCloseTo(zoom / 100, 2);
    expect(Math.abs(at.menuLeft - at.cellLeft), `menu ${at.menuLeft} vs cell ${at.cellLeft}`).toBeLessThan(6);
    expect(Math.abs(at.menuTop - at.cellBottom), `menu ${at.menuTop} vs cell bottom ${at.cellBottom}`).toBeLessThan(6);
  });
}

for (const zoom of [100, 150]) {
  test(`a validation input message sits under its cell at ${zoom}%`, async ({ page }) => {
    await page.goto("/editor.html?sample=demo");
    await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
    await page.evaluate((z) => {
      const ed = window.opencalcEditor;
      const api = ed.wasmApi();
      api.session_new();
      api.session_set_list_validation(0, 5, 2, 5, 2, ["yes", "no"]);
      api.session_set_validation_messages(0, 5, 2, 5, 2, "stop", ["", "", "Answer", "Pick yes or no"], false);
      ed.runCommand(`view.zoom.${z}`);
      ed.selectForTest(5, 2);
    }, zoom);
    const box = page.locator("#dv-prompt");
    await expect(box).toBeVisible();
    const at = await page.evaluate(() => {
      const ed = window.opencalcEditor;
      const b = ed.cellBoxForTest(5, 2);
      const g = document.getElementById("grid").getBoundingClientRect();
      const zoom = ed.scrollStateForTest().zoom;
      const m = document.getElementById("dv-prompt").getBoundingClientRect();
      return { cellLeft: g.left + b.x * zoom, cellBottom: g.top + (b.y + b.h) * zoom, left: m.left, top: m.top };
    });
    expect(Math.abs(at.left - at.cellLeft), `message ${at.left} vs cell ${at.cellLeft}`).toBeLessThan(6);
    expect(at.top - at.cellBottom, `message ${at.top} vs cell bottom ${at.cellBottom}`).toBeGreaterThanOrEqual(0);
    expect(at.top - at.cellBottom).toBeLessThan(10);
  });
}
