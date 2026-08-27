// A spreadsheet you cannot scroll with a finger is not usable on a phone.
//
// The grid is a canvas the editor scrolls itself, and every one of its
// ancestors is `overflow: clip`, `hidden` or `visible` — so there is nothing
// for a browser to scroll natively. There were also no touch or pointer
// listeners anywhere in the webapp. The two facts together meant a phone user
// could see the first screenful of a workbook and nothing else, ever: tap
// selected a cell (the browser synthesises a click) and double-tap opened the
// editor (likewise), so the app looked like it worked right up until you tried
// to reach row 30.
//
// It is asserted with real touch input through CDP rather than synthesised
// TouchEvents. A hand-built `new TouchEvent(...)` proved this "broken" before
// the handlers existed and would have proved it "fixed" just as readily —
// dispatching an event is not the same as the browser delivering one.

import { expect, test } from "@playwright/test";

test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 80; r += 1) a.session_set_cell(0, r, 0, `row${r}`);
  });
}

const scroll = (page) => page.evaluate(() => window.opencalcEditor.scrollStateForTest());

/** A real finger: press, glide, release — delivered by the browser, not by JS. */
async function swipe(page, { dx = 0, dy = 0, steps = 8 } = {}) {
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const x0 = box.x + box.width / 2;
  const y0 = box.y + box.height * 0.75;
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: x0, y: y0 }] });
  for (let i = 1; i <= steps; i += 1) {
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [{ x: x0 + (dx * i) / steps, y: y0 + (dy * i) / steps }],
    });
    await page.waitForTimeout(16);
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(250);
}

test("dragging a finger up scrolls the grid down", async ({ page }) => {
  await boot(page);
  expect((await scroll(page)).scrollY).toBe(0);
  await swipe(page, { dy: -200 });
  const after = await scroll(page);
  // The content follows the finger, so a 200px drag upward moves roughly 200px
  // of sheet. Loose bounds: what matters is that it moved, and by about the
  // distance dragged rather than one row or the whole sheet.
  expect(after.scrollY, "an upward swipe scrolls down").toBeGreaterThan(100);
  expect(after.scrollY, "and not wildly further than the finger travelled").toBeLessThan(320);
});

test("dragging sideways scrolls sideways", async ({ page }) => {
  await boot(page);
  await swipe(page, { dx: -160 });
  expect((await scroll(page)).scrollX, "a leftward swipe scrolls right").toBeGreaterThan(60);
});

test("the grid cannot be dragged above its first row", async ({ page }) => {
  await boot(page);
  await swipe(page, { dy: 300 });
  const after = await scroll(page);
  expect(after.scrollY, "already at the top, so it stays there").toBe(0);
  expect(after.scrollX).toBe(0);
});

test("a tap still selects, and a swipe does not", async ({ page }) => {
  await boot(page);
  const sel = () => page.evaluate(() => window.opencalcEditor.selectionRectForTest());
  const box = await page.locator("#grid").boundingBox();

  await page.touchscreen.tap(box.x + 220, box.y + 130);
  const tapped = await sel();
  expect(tapped.r0, "a tap picks a cell").toBeGreaterThan(0);

  // The browser synthesises a click where the finger lifts. Without suppressing
  // it, every swipe would also move the selection to wherever the drag ended —
  // scrolling a sheet would silently change which cell you are on.
  await swipe(page, { dy: -200 });
  const after = await sel();
  expect(after, "a pan must not move the selection").toEqual(tapped);
});

test("pinching zooms the sheet, not the page", async ({ page }) => {
  await boot(page);
  const zoom = () => page.evaluate(() => window.opencalcEditor.scrollStateForTest().zoom);
  expect(await zoom()).toBe(1);

  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;

  // Two fingers starting 60px apart and spreading to 180px — a zoom in.
  const pair = (gap) => [
    { x: cx - gap / 2, y: cy, id: 1 },
    { x: cx + gap / 2, y: cy, id: 2 },
  ];
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: pair(60) });
  for (const gap of [80, 110, 140, 170, 180]) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: pair(gap) });
    await page.waitForTimeout(16);
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(250);

  // Tripling the spread should zoom in substantially. The engine clamps its own
  // range, so this asserts the direction and that it moved, not an exact factor.
  expect(await zoom(), "spreading two fingers zooms in").toBeGreaterThan(1.2);
});
