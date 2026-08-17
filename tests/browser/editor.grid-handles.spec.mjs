// The two grid gestures a user reaches for before finding any menu:
// revealing a hidden band by clicking its handle, and dragging a freeze into
// existence from the corner.
//
// Both are mouse gestures on a canvas, so a browser is the only place they can
// be asserted at all — nothing in Rust can tell you that a click armed a resize
// instead of revealing a column.

import { expect, test } from "@playwright/test";

/// The editor's own view of where its chrome is. A test that guessed these
/// coordinates would be hunting a five-pixel target and would go flaky the
/// first time a header default changed.
const handles = (page) =>
  page.evaluate(() => window.opencalcEditor.gridHandlesForTest());

async function boot(page) {
  await page.goto("/editor.html");
  // The signal the smoke suite uses: the engine writes its version here once
  // the WebAssembly module is live.
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// Canvas-relative pixels to page coordinates, honouring zoom.
async function at(page, x, y, zoom) {
  const box = await page.locator("#grid").boundingBox();
  return { x: box.x + x * zoom, y: box.y + y * zoom };
}

/// **Clicking a hidden band's handle brings it back.**
///
/// A hidden column has zero width, so its edge sits exactly on its neighbour's
/// — which means the resize hit test matches every one of these handles. The
/// click armed a resize of the hidden column instead of revealing it, and the
/// handle was drawn as a control that did nothing. The only way back was a
/// double-click, which a comment in the source already claimed a single click
/// had handled.
test("a hidden column is revealed by clicking its handle", async ({ page }) => {
  await boot(page);

  // Hide column C through the menu, as a user would.
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 2));
  // The menu bar and its items both carry `data-oc-label`, which is the label
  // before translation and mnemonic markup touch it.
  await page.locator('[data-oc-label="Data"]').click();
  await page.locator('[data-oc-label="Hide columns"]').click();

  const hidden = await handles(page);
  expect(hidden.hiddenCols.length, "no handle was drawn for the hidden column").toBe(1);
  expect(hidden.hiddenCols[0].from).toBe(2);

  // One click on the handle.
  const point = await at(page, hidden.hiddenCols[0].x, 12, hidden.zoom);
  await page.mouse.click(point.x, point.y);

  const after = await handles(page);
  expect(after.hiddenCols.length, "the column is still hidden after clicking its handle").toBe(0);
});

/// **A freeze can be created by dragging, not only chosen from a menu.**
///
/// The divider is drawn at the freeze line, and there is no line at zero — so
/// every drag gesture operated on a freeze somebody had already made through
/// the menu, and no gesture made one. The handle a user goes looking for first
/// did not exist.
test("dragging the corner handle freezes columns", async ({ page }) => {
  await boot(page);

  const before = await handles(page);
  expect(before.freeze.fc).toBe(0);
  expect(before.freezeHandles.col, "no column freeze handle in the corner").not.toBeNull();

  const from = await at(page, before.freezeHandles.col.x, before.freezeHandles.col.y, before.zoom);
  // Out to somewhere in the third column — far enough that the drop lands well
  // inside the body rather than on the boundary it started from.
  const to = await at(page, 260, before.freezeHandles.col.y + 40, before.zoom);

  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps: 8 });
  await page.mouse.up();

  const after = await handles(page);
  expect(after.freeze.fc, "the drag did not freeze any columns").toBeGreaterThan(0);
  // And the handle is gone, because there is now a divider to drag instead.
  expect(after.freezeHandles.col).toBeNull();
});

/// **The corner still selects the whole sheet.**
///
/// The handles are carved out of the select-all box, so this is the thing they
/// could plausibly have broken.
test("the corner box still selects the whole sheet", async ({ page }) => {
  await boot(page);

  const geometry = await handles(page);
  // Middle of the corner, clear of both handles.
  const point = await at(page, 12, 8, geometry.zoom);
  await page.mouse.click(point.x, point.y);

  const after = await handles(page);
  expect(after.selKind, "the corner no longer selects everything").toBe("all");
  // And it certainly must not have made a freeze.
  expect(after.freeze.fc).toBe(0);
  expect(after.freeze.fr).toBe(0);
});
