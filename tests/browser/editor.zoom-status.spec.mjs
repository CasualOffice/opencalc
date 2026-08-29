// The zoom level must be readable, and changeable, without opening a menu.
//
// `docs/47` ranks this the #1 daily miss and `docs/12` §8 ranks it the best
// cost-to-benefit item on the whole switching-blocker list. Zoom itself was
// never missing: `View ▸ Zoom` has five presets, `Ctrl+Alt+0` resets, and both
// `Ctrl`+wheel and a trackpad pinch drive `setZoom()`. What was missing is any
// place that *says* the level — so a user who pinched by accident could see the
// grid was wrong and had no way to read how wrong, and no control to undo it
// without finding a submenu two levels into the menu bar.
//
// # Why these assertions and not "the widget exists"
//
// The sweep row this closes (`ux-sweep.mjs --only "zoom level is visible"`)
// looks for `/\d{2,3}\s*%/` in the status bar's text. That is satisfied by the
// static markup alone: a page whose JavaScript never ran still renders the
// literal `100%` in the button. So the sweep row passing is necessary and not
// sufficient, and every test below turns on the readout **tracking a zoom that
// actually happened** — which is the half that can regress silently.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  // The sample workbook is not what any of this measures, but `session_new()`
  // is cheap and it is the mistake this repository has already made once.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
}

const zoomOf = (page) =>
  page.evaluate(() => window.opencalcEditor.scrollStateForTest().zoom);

test("the status bar shows the zoom level without opening anything", async ({ page }) => {
  await boot(page);
  // Visible, not merely present: a control the user has to reveal is the menu
  // this row exists to replace.
  await expect(page.locator("#zoom-level")).toBeVisible();
  await expect(page.locator("#zoom-level")).toHaveText("100%");
});

test("the readout follows a zoom the user did not make from this widget", async ({ page }) => {
  await boot(page);
  // Through the menu's own path — `setZoom` is what `View ▸ Zoom ▸ 150%`,
  // `Ctrl`+wheel and a pinch all call. A widget that only tracked its own two
  // buttons would be right here and stale for every other route to zoom, which
  // is the failure mode worth a test.
  await page.evaluate(() => window.opencalcEditor.setZoom(1.5));
  await expect(page.locator("#zoom-level")).toHaveText("150%");
  await expect(page.locator("#zoom-slider")).toHaveValue("150");
});

test("the plus and minus buttons change the zoom the grid is drawn at", async ({ page }) => {
  await boot(page);
  const before = await zoomOf(page);
  expect(before).toBeCloseTo(1, 2);

  await page.locator("#zoom-in").click();
  const zoomedIn = await zoomOf(page);
  // The engine state, not just the label: a readout that moves while the grid
  // does not is worse than no readout.
  expect(zoomedIn).toBeGreaterThan(before);
  await expect(page.locator("#zoom-level")).toHaveText(`${Math.round(zoomedIn * 100)}%`);

  await page.locator("#zoom-out").click();
  expect(await zoomOf(page)).toBeLessThan(zoomedIn);
});

test("clicking the percentage returns to 100%", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.setZoom(0.5));
  await expect(page.locator("#zoom-level")).toHaveText("50%");

  await page.locator("#zoom-level").click();
  expect(await zoomOf(page)).toBeCloseTo(1, 2);
  await expect(page.locator("#zoom-level")).toHaveText("100%");
});

test("the ends of the range disable the button that cannot move", async ({ page }) => {
  await boot(page);
  // `ZOOM_MAX` is 2 and `ZOOM_MIN` is 0.25. A button still lit at the ceiling
  // and doing nothing reads as a broken editor.
  await page.evaluate(() => window.opencalcEditor.setZoom(2));
  await expect(page.locator("#zoom-in")).toBeDisabled();
  await expect(page.locator("#zoom-out")).toBeEnabled();

  await page.evaluate(() => window.opencalcEditor.setZoom(0.25));
  await expect(page.locator("#zoom-out")).toBeDisabled();
  await expect(page.locator("#zoom-in")).toBeEnabled();
});
