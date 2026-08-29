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

test("a long press opens the cell menu", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const x = box.x + 220;
  const y = box.y + 130;

  // Without this there is no cut, copy, paste, insert or delete on a phone at
  // all: every one of them lives in the context menu, and a touch device has no
  // right button. The handler behind it already understands headers, the
  // corner and cells — a long press raises the same event rather than growing a
  // second, thinner menu that drifts from it.
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
  await page.waitForTimeout(700);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  await expect(page.locator("#sheet-ctx")).toBeVisible();
  // On the cell pressed, not on whatever was selected beforehand.
  const sel = await page.evaluate(() => window.opencalcEditor.selectionRectForTest());
  expect(sel.r0, "the press selects what it opened on").toBeGreaterThan(0);
});

test("a long press that turns into a drag pans instead", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const x = box.x + 220;
  const y0 = box.y + 300;

  // Holding still and then dragging is a scroll, not a menu. Getting this wrong
  // means a menu appears mid-swipe and the sheet stops moving under the finger.
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y: y0 }] });
  await page.waitForTimeout(250);
  for (const dy of [-30, -70, -120, -170]) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y0 + dy }] });
    await page.waitForTimeout(16);
  }
  await page.waitForTimeout(600); // well past the press threshold
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(200);

  await expect(page.locator("#sheet-ctx")).toHaveCount(0);
  expect((await scroll(page)).scrollY, "it scrolled instead").toBeGreaterThan(50);
});

test("a flick keeps gliding after the finger lifts", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const x = box.x + box.width / 2;
  const y0 = box.y + box.height * 0.8;

  // A fast flick, released while still moving. The steps are large because the
  // throw is scaled by how recently the finger was last seen moving, and a CI
  // runner delivers the release later than a phone does — a gentle flick would
  // measure the runner's scheduling rather than the editor's inertia.
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y: y0 }] });
  for (const dy of [-60, -130, -200, -270]) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y0 + dy }] });
    await page.waitForTimeout(16);
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  const atRelease = (await scroll(page)).scrollY;
  await page.waitForTimeout(700);
  const settled = (await scroll(page)).scrollY;

  // Without inertia the sheet stops dead the instant the finger leaves, which
  // is the single thing that makes a touch surface feel like a web page rather
  // than an application.
  expect(settled, "the flick carries on after release").toBeGreaterThan(atRelease + 30);
});

test("a flick still throws when the release arrives late", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const x = box.x + box.width / 2;
  const y0 = box.y + box.height * 0.8;

  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y: y0 }] });
  for (const dy of [-60, -130, -200, -270]) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y0 + dy }] });
    await page.waitForTimeout(16);
  }
  // The release, 120ms after the last movement. This is the case that failed on
  // CI and would fail on a slow phone: the finger was plainly still moving, but
  // a hard 90ms cut-off decided the flick was stale and dropped it entirely.
  // The scroll then stopped dead at the exact pixel the finger left, which a
  // user reads as the app being laggy rather than as a threshold being missed.
  await page.waitForTimeout(120);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  const atRelease = (await scroll(page)).scrollY;
  await page.waitForTimeout(700);
  const settled = (await scroll(page)).scrollY;
  expect(settled, "a late release still throws, only weaker").toBeGreaterThan(atRelease + 10);
});

test("a glide is a scroll, so the accessibility mirror is not rebuilt per frame", async ({ page }) => {
  await boot(page);
  // `viewIsMoving()` is called from the two paths a finger or a wheel drives,
  // and a glide is neither: it is the throw after the finger has already gone,
  // running on its own rAF. So the mirror's deferral never saw it, and one
  // fling rebuilt hundreds of DOM nodes on all forty of its frames — the whole
  // per-frame cost `PERF-D-01` removed, still present on the platform with the
  // least frame budget to spare. Measured at 40 rebuilds per fling before the
  // glide step started declaring itself, and 4 after.
  const rebuilds = () => page.evaluate(() => window.opencalcEditor.a11yRebuildCountForTest());
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  const x = box.x + box.width / 2;
  const y0 = box.y + box.height * 0.8;

  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y: y0 }] });
  for (let i = 1; i <= 10; i += 1) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y0 - i * 30 }] });
    await page.waitForTimeout(16);
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  const atRelease = await rebuilds();
  const scrollAtRelease = (await scroll(page)).scrollY;
  await page.waitForTimeout(1500);

  expect((await scroll(page)).scrollY, "nothing glided, so this proves nothing")
    .toBeGreaterThan(scrollAtRelease + 20);
  const during = (await rebuilds()) - atRelease;
  // Bounded by the staleness ceiling rather than by zero: a fling is still
  // motion, and `A11Y-01` is the reason the mirror has to catch up during it.
  expect(during, `${during} mirror rebuilds during one glide`).toBeLessThanOrEqual(10);
});
