// The keyboard's context menu opens at the active cell (fit-gap XL-UX-015).
//
// Excel: the ContextMenu key and Shift+F10 both open the cell menu *at the
// active cell* and leave the selection alone. OpenCalc treated the key's
// synthetic `contextmenu` as a right-click at the mouse pointer — B2 active,
// pointer over K16, and the menu opened at K16 with K16 selected — and Shift+F10
// opened nothing at all.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const screenBox = (page, r, c) => page.evaluate(([r, c]) => {
  const b = window.opencalcEditor.cellBoxForTest(r, c);
  const g = document.getElementById("grid").getBoundingClientRect();
  return { x: g.left + b.x, y: g.top + b.y, w: b.w, h: b.h };
}, [r, c]);
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());

for (const [name, chord] of [["the ContextMenu key", "ContextMenu"], ["Shift+F10", "Shift+F10"]]) {
  test(`${name} opens the cell menu at the active cell and keeps the selection`, async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));   // B2
    // The pointer parked over K16, where the old handler opened the menu.
    const far = await screenBox(page, 15, 10);
    await page.mouse.move(far.x + far.w / 2, far.y + far.h / 2);
    await page.locator("#grid").focus();
    await page.keyboard.press(chord);

    const menu = page.locator("#sheet-ctx");
    await expect(menu, "no menu opened").toBeVisible();
    expect(await sel(page), "the selection moved").toEqual({ r0: 1, c0: 1, r1: 1, c1: 1 });
    await expect(page.locator("#cell-ref")).toHaveValue("B2");

    const b2 = await screenBox(page, 1, 1);
    const m = await menu.boundingBox();
    // Anchored to B2's box, not to the pointer. Horizontally it starts inside
    // B2; vertically a menu taller than the room below is moved up to fit, so
    // the test is that its span reaches B2's row rather than where it starts.
    expect(m.x - b2.x, "menu is not at the active cell horizontally").toBeGreaterThanOrEqual(0);
    expect(m.x - b2.x, "menu is not at the active cell horizontally").toBeLessThanOrEqual(b2.w);
    expect(m.y, "menu starts below the active cell").toBeLessThanOrEqual(b2.y + b2.h + 2);
    expect(m.y + m.height, "menu ends above the active cell").toBeGreaterThanOrEqual(b2.y);

    // Keyboard-driven from here: the first item has focus, arrows move it,
    // Escape closes and gives the grid its focus back.
    const focused = () => page.evaluate(() => {
      const a = document.activeElement;
      return a && a.closest("#sheet-ctx") ? a.textContent.trim() : null;
    });
    expect(await focused(), "the first menu item has no focus").toBe("Cut");
    await page.keyboard.press("ArrowDown");
    expect(await focused()).toBe("Copy");
    await page.keyboard.press("ArrowUp");
    expect(await focused()).toBe("Cut");
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    expect(await page.evaluate(() => document.activeElement?.id)).toBe("grid");
    expect(await sel(page)).toEqual({ r0: 1, c0: 1, r1: 1, c1: 1 });
  });
}

test("Enter on a focused menu item runs it", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.locator("#grid").focus();
  await page.keyboard.press("Shift+F10");
  await expect(page.locator("#sheet-ctx")).toBeVisible();
  // Down to "Format cells…" and run it.
  for (let i = 0; i < 30; i++) {
    const t = await page.evaluate(() => document.activeElement?.textContent?.trim());
    if (t === "Format cells…") break;
    await page.keyboard.press("ArrowDown");
  }
  await page.keyboard.press("Enter");
  await expect(page.locator("#oc-modal")).toBeVisible();
  await expect(page.locator("#oc-modal-title")).toHaveText(/Format/i);
});

test("a real right-click still targets the cell under the pointer", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  const k16 = await screenBox(page, 15, 10);
  await page.mouse.click(k16.x + k16.w / 2, k16.y + k16.h / 2, { button: "right" });
  await expect(page.locator("#sheet-ctx")).toBeVisible();
  expect(await sel(page)).toEqual({ r0: 15, c0: 10, r1: 15, c1: 10 });
});

/// Where the platform raises `contextmenu` for the key itself (Windows), the
/// event carries the *pointer's* coordinates — that is the K16 of the report.
/// Pointer Events mark a keyboard origin with pointerId -1 and no pointerType.
test("a keyboard-raised contextmenu at the pointer still opens at the active cell", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  const far = await screenBox(page, 15, 10);
  await page.evaluate(({ x, y }) => {
    document.getElementById("grid").dispatchEvent(new PointerEvent("contextmenu", {
      bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, pointerId: -1, pointerType: "",
    }));
  }, { x: far.x + far.w / 2, y: far.y + far.h / 2 });
  await expect(page.locator("#sheet-ctx")).toBeVisible();
  expect(await sel(page), "the selection moved to the pointer").toEqual({ r0: 1, c0: 1, r1: 1, c1: 1 });
  const m = await page.locator("#sheet-ctx").boundingBox();
  const b2 = await screenBox(page, 1, 1);
  expect(m.x - b2.x, "menu opened at the pointer").toBeLessThanOrEqual(b2.w);
});
