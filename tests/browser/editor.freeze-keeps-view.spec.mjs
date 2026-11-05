// Freezing from the menu keeps the view where it is (PIPE-09).
//
// Excel's Freeze Top Row / Freeze First Column / Unfreeze change where the
// panes split, not what the user is looking at. The menu path here reset both
// scroll offsets to zero, so freezing the first column while reading row 300
// threw the user back to A1 — while dragging the freeze divider, the other way
// to the same change, left the view alone. The two paths now agree.
//
// "Freeze up to selection" is the one case that still moves, on the axis it
// freezes: this model's frozen pane always starts at row 1 / column A, so the
// active cell can only sit just below the split when that axis is unscrolled.
// The axis it does not freeze keeps its scroll.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    for (let r = 0; r < 400; r += 1) for (let c = 0; c < 40; c += 3) a.session_set_cell(0, r, c, String(r * 100 + c));
    ed.selectForTest(0, 0);
    ed.draw();
  });
  await page.locator("#grid").focus();
}

const scroll = (page) => page.evaluate(() => {
  const s = window.opencalcEditor.scrollStateForTest();
  return { x: s.scrollX, y: s.scrollY };
});
const run = (page, id) => page.evaluate((id) => { window.opencalcEditor.runCommand(id); }, id);

/// Scroll the grid the way a user does, with the wheel over it.
async function scrollBy(page, dx, dy) {
  const g = await page.locator("#grid").boundingBox();
  await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2);
  await page.mouse.wheel(dx, dy);
  await page.waitForTimeout(150);
}

for (const id of ["view.freeze.top-row", "view.freeze.first-column"]) {
  test(`${id} keeps the scrolled view`, async ({ page }) => {
    await boot(page);
    // One axis per wheel event: the wheel handler scrolls the dominant one.
    await scrollBy(page, 0, 2400);
    await scrollBy(page, 400, 0);
    const before = await scroll(page);
    expect(before.y, "the grid scrolled down").toBeGreaterThan(1000);
    expect(before.x, "and across").toBeGreaterThan(100);

    await run(page, id);
    await page.waitForTimeout(150);
    const fz = await page.evaluate(() => ({ ...window.opencalcEditor.state.freeze }));
    expect(id.endsWith("top-row") ? fz.fr : fz.fc, "the freeze took").toBe(1);
    expect(await scroll(page), "freezing did not jump back to A1").toEqual(before);
  });
}

test("unfreezing keeps the scrolled view", async ({ page }) => {
  await boot(page);
  await run(page, "view.freeze.top-row");
  await scrollBy(page, 0, 2400);
  const before = await scroll(page);
  expect(before.y).toBeGreaterThan(1000);
  await run(page, "view.freeze.unfreeze");
  await page.waitForTimeout(150);
  expect(await scroll(page)).toEqual(before);
});

test("freezing up to the selection keeps the scroll of the axis it does not freeze", async ({ page }) => {
  await boot(page);
  await scrollBy(page, 0, 2400);
  const before = await scroll(page);
  // Column A, a row in view: rows freeze above it, no columns do.
  const row = await page.evaluate(() => window.opencalcEditor.firstBodyRow() + 3);
  await page.evaluate((row) => window.opencalcEditor.selectForTest(row, 0), row);
  await page.evaluate(() => window.opencalcEditor.state.scrollX = 0);
  await scrollBy(page, 400, 0);
  const across = await scroll(page);
  expect(across.x).toBeGreaterThan(100);
  expect(across.y).toBe(before.y);

  await run(page, "view.freeze.up-to-selection");
  await page.waitForTimeout(150);
  const fz = await page.evaluate(() => ({ ...window.opencalcEditor.state.freeze }));
  expect(fz.fr).toBe(row);
  expect(fz.fc).toBe(0);
  const after = await scroll(page);
  expect(after.x, "no column was frozen, so the horizontal scroll stays").toBe(across.x);
});
