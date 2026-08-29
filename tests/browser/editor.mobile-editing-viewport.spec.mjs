// You have to be able to see the cell you are typing into.
//
// A software keyboard does not resize the page. It shrinks the **visual**
// viewport and leaves `window.innerHeight` — the layout viewport — alone.
// `.editor-body` is `height: 100vh`, so the grid keeps its full height and the
// bottom of it is simply covered over. Nothing in `webapp/` referenced
// `visualViewport` at all.
//
// Measured before the fix, on a 390×844 phone, editing a cell near the foot of
// the grid:
//
//     {"step":"editing a cell near the bottom of the grid",
//      "rect":{"x":174,"y":738,"w":64,"h":20},
//      "keyboardTop_iOS336":508,"occludedByIosKeyboard":true,
//      "keyboardTop_Android270":574,"occludedByAndroidKeyboard":true}
//
// The in-cell editor sat 230px below where an iOS keyboard starts. You type
// blind, and the formula bar is the only thing you can read.
//
// **Why the vehicle is a page scale and not a keyboard.** No browser automation
// can raise a software keyboard. But a keyboard is not special: it is one of
// two ways the visual viewport comes to be shorter than the layout viewport,
// and a pinch or page zoom is the other. Both hand the page the same signal —
// `innerHeight - (visualViewport.offsetTop + visualViewport.height)` — through
// the same `visualViewport` `resize` event, and both want the same answer. So
// the fix is written as "keep the cell being edited inside what is visible",
// and `Emulation.setPageScaleFactor` drives the real code path with a real
// browser-delivered event rather than a hand-built one.
//
// What that leaves uncovered is stated rather than papered over: these tests
// prove the editor reacts to a shrinking visual viewport, not that iOS Safari
// reports its keyboard through `visualViewport` — which it does, and which
// nothing available here can demonstrate.

import { expect, test } from "@playwright/test";

test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    // Enough sheet below the viewport that the grid has somewhere to scroll to.
    for (let r = 0; r < 400; r += 1) a.session_set_cell(0, r, 0, `row${r}`);
  });
  await page.waitForTimeout(300);
}

/** Where the in-cell editor is, and where the visible area ends. */
const measure = (page) => page.evaluate(() => {
  const t = document.querySelector("#inline-edit");
  const r = t.getBoundingClientRect();
  const vv = window.visualViewport;
  const visibleBottom = Math.min(window.innerHeight, vv.offsetTop + vv.height);
  return {
    shown: getComputedStyle(t).display !== "none" && r.height > 0,
    top: +r.top.toFixed(1), bottom: +r.bottom.toFixed(1),
    visibleBottom: +visibleBottom.toFixed(1),
    innerHeight: window.innerHeight,
    vv: { h: +vv.height.toFixed(1), offsetTop: +vv.offsetTop.toFixed(1), scale: +vv.scale.toFixed(3) },
    occluded: r.bottom > visibleBottom + 0.5,
    scrollY: window.opencalcEditor.scrollStateForTest().scrollY,
  };
});

/** Open the in-cell editor on a cell low in the grid, by finger. */
async function editLowCell(page, cdp) {
  const box = await page.locator("#grid").boundingBox();
  const x = box.x + 120;
  const y = box.y + box.height - 60;
  for (const _ of [0, 1]) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await page.waitForTimeout(60);
  }
  await page.waitForTimeout(350);
}

test("the visual viewport shrinking under an open editor brings the cell back into view", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  await editLowCell(page, cdp);

  const before = await measure(page);
  expect(before.shown, "the editor never opened, so this would prove nothing").toBe(true);
  expect(before.occluded, "nothing covers it yet").toBe(false);

  // The keyboard's stand-in. A real page-scale change, delivered by the browser
  // as a `visualViewport` resize — not a synthesised event.
  await cdp.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1.6 });
  await page.waitForTimeout(400);

  const after = await measure(page);
  expect(
    after.innerHeight - after.visibleBottom,
    `only ${(after.innerHeight - after.visibleBottom).toFixed(1)}px went out of view — nothing was covered, so this proves nothing`,
  ).toBeGreaterThan(150);
  expect(before.bottom, "the editor was above the fold to begin with").toBeGreaterThan(after.visibleBottom);
  expect(
    after.occluded,
    `the editor is at ${after.bottom} and the visible area ends at ${after.visibleBottom}`,
  ).toBe(false);
  expect(after.scrollY, "it came into view by scrolling the sheet").toBeGreaterThan(before.scrollY);
});

test("an edit begun while the viewport is already short opens somewhere visible", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);

  // The ordinary case once a keyboard is up: Enter moves to the next row and
  // the editor reopens. Nothing resizes, so a resize listener alone never fires.
  await cdp.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1.6 });
  await page.waitForTimeout(300);
  const room = await page.evaluate(() => window.innerHeight - Math.min(window.innerHeight, window.visualViewport.offsetTop + window.visualViewport.height));
  expect(room, `only ${room.toFixed(1)}px hidden — this would prove nothing`).toBeGreaterThan(150);

  await editLowCell(page, cdp);
  const m = await measure(page);
  expect(m.shown, "the editor never opened").toBe(true);
  expect(m.occluded, `editor bottom ${m.bottom}, visible to ${m.visibleBottom}`).toBe(false);
});

test("a full-height viewport leaves the sheet exactly where it was", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  // The guard against a fix that just scrolls on every edit: with nothing
  // covering the grid, opening an editor must not move the sheet at all.
  const at = await page.evaluate(() => window.opencalcEditor.scrollStateForTest().scrollY);
  await editLowCell(page, cdp);
  const m = await measure(page);
  expect(m.shown).toBe(true);
  expect(m.occluded).toBe(false);
  expect(await page.evaluate(() => window.opencalcEditor.scrollStateForTest().scrollY),
    "nothing was in the way, so nothing should have scrolled").toBe(at);
});
