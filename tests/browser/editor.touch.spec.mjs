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

// --- Range selection by handle (UX-MOB-06) --------------------------------
//
// A drag on this grid pans, because on a phone a drag is the scroll gesture and
// nothing else can be. So before the handles there was **no route to a range at
// all**: selecting `A1:C5` meant typing it into the Name Box, and everything
// downstream of a range — sum, sort, chart, format, fill, copy — was reachable
// only by someone who already knew A1 notation.
//
// The tests below press the **visual corner of the selection**, not a point
// derived from the handle's own geometry, and the block they assert on is read
// through `selectionRectForTest` and the Name Box. Both existed before this
// work: with the handles removed these still run, and they fail by panning the
// sheet and leaving the selection one cell — which is the actual old behaviour
// rather than a missing export.

/// The screen point at a cell's bottom-right corner, from the engine's own
/// column and row offsets. Deliberately not `touchHandlesForTest()`: a test
/// that asks the feature where to press cannot fail when the feature is absent.
async function cellCorner(page, row, col, which = "br") {
  const g = await page.evaluate(([r, c, w]) => {
    const e = window.opencalcEditor;
    const a = e.wasmApi();
    const s = e.scrollStateForTest();
    const col0 = w === "br" ? c + 1 : c;
    const row0 = w === "br" ? r + 1 : r;
    return {
      x: s.bodyX0 + a.session_col_offset_px(0, col0) - s.scrollX,
      y: s.bodyY0 + a.session_row_offset_px(0, row0) - s.scrollY,
      zoom: s.zoom,
    };
  }, [row, col, which]);
  const box = await page.locator("#grid").boundingBox();
  return { x: box.x + g.x * g.zoom, y: box.y + g.y * g.zoom };
}

/// A finger pressing one point, dragging to another, and lifting.
async function fingerDrag(page, from, to, { steps = 8, hold = 0, onMove } = {}) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [from] });
  if (hold) await page.waitForTimeout(hold);
  for (let i = 1; i <= steps; i += 1) {
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [{ x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps }],
    });
    await page.waitForTimeout(16);
  }
  if (onMove) await onMove();
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(250);
}

const rect = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());
const nameBox = (page) => page.locator("#cell-ref").inputValue();

test("a finger can select a range by dragging the selection's corner", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 1));
  // A tap first, so the grid has seen a finger exactly as a real session would.
  const box = await page.locator("#grid").boundingBox();
  await page.touchscreen.tap(box.x + 150, box.y + 120);
  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 1));
  expect(await rect(page), "one cell to begin with").toMatchObject({ r0: 4, c0: 1, r1: 4, c1: 1 });

  const corner = await cellCorner(page, 4, 1);
  await fingerDrag(page, corner, { x: corner.x + 150, y: corner.y + 140 });

  const after = await rect(page);
  // The whole point of the row: a range, from a gesture, with no typing.
  expect(after.r1, "the block grew downward").toBeGreaterThan(4);
  expect(after.c1, "and rightward").toBeGreaterThan(1);
  expect({ r0: after.r0, c0: after.c0 }, "the far corner stayed put").toEqual({ r0: 4, c0: 1 });
  // A pan would have moved the sheet instead. It must not have.
  expect((await scroll(page)).scrollY, "the sheet did not scroll under the drag").toBe(0);
});

test("the corner a finger is not holding stays the active cell", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.touchscreen.tap(box.x + 150, box.y + 120);
  // A block already selected, D7:F10 in A1 terms.
  await page.evaluate(() => {
    const e = window.opencalcEditor;
    e.selectForTest(6, 3);
    e.extendSelectionForTest(9, 5);
  });
  expect(await rect(page), "D7:F10 to begin with").toMatchObject({ r0: 6, c0: 3, r1: 9, c1: 5 });

  // Drag the *top-left* handle up and left. Excel and Sheets both leave the
  // active cell on the corner that did not move, which is now the bottom-right
  // — and typing goes to the active cell, so getting this backwards writes into
  // the wrong end of the block the user is looking at.
  const tl = await cellCorner(page, 6, 3, "tl");
  await fingerDrag(page, tl, { x: tl.x - 100, y: tl.y - 60 });

  const after = await rect(page);
  expect(after.r0, "the top edge came up").toBeLessThan(6);
  expect(after.c0, "and the left edge came left").toBeLessThan(3);
  expect({ r1: after.r1, c1: after.c1 }, "the bottom-right stayed").toEqual({ r1: 9, c1: 5 });
  expect(await nameBox(page), "the active cell is the corner that stayed").toBe("F10");
});

test("the name box counts the block while a handle is being dragged", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.touchscreen.tap(box.x + 150, box.y + 120);
  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 1));

  const corner = await cellCorner(page, 4, 1);
  let midDrag = null;
  await fingerDrag(page, corner, { x: corner.x + 150, y: corner.y + 140 }, {
    // Excel's readout, and the only thing on a phone that says how big the block
    // is while it is being built — the row/column headers are too small to count
    // and the block itself runs off the screen.
    onMove: async () => { midDrag = await nameBox(page); },
  });
  expect(midDrag, "the size, while the finger is still down").toMatch(/^\d+R x \d+C$/);
  expect(await nameBox(page), "and the active cell once it lifts").toBe("B5");
});

test("a tap on a handle does not move the selection", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.touchscreen.tap(box.x + 150, box.y + 120);
  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 1));
  const before = await rect(page);

  // A finger that lands on the handle and lifts without travelling. The browser
  // synthesises a click there, and the cell under a handle is the *next* one
  // over — so left alone this reads as "tapping the corner of my selection
  // moves it diagonally", which is worse than the handle not existing.
  //
  // Eight pixels *outside* the corner: on the handle as it is drawn, and past
  // the two things that used to happen there instead. Measured on the build
  // without handles, sweeping the offset:
  //
  //     +0 .. +5   selection unchanged — the finger landed in the *fill
  //                handle's* 5px mouse hit zone, silently arming a drag-fill
  //                that a touch device has no way to finish
  //     +8 and out selection jumps to C6 — "tapping the corner of my block
  //                moves it diagonally"
  //
  // Anything inside +5 therefore passes without the handles and proves nothing,
  // which is what the first draft of this test did.
  const c0 = await cellCorner(page, 4, 1);
  const corner = { x: c0.x + 8, y: c0.y + 8 };
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [corner] });
  await page.waitForTimeout(120);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(300);

  expect(await rect(page), "the selection is exactly where it was").toEqual(before);
});

test("a handle is drawn where it is grabbed, even against the headers", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.touchscreen.tap(box.x + 150, box.y + 120);
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));

  // Selected on A1, the top-left handle's unclamped centre is (HW − r, HH − r):
  // inside the header corner, where the body clip erases it — while the hit
  // test went on answering for a finger there. Invisible *and* live is the worst
  // pair: nothing says the handle is there, and pressing the headers starts a
  // range drag nobody asked for. Caught by screenshotting A1, not by reading.
  const h = await page.evaluate(() => window.opencalcEditor.touchHandlesForTest());
  const s = await page.evaluate(() => window.opencalcEditor.scrollStateForTest());
  expect(h.tl, "A1 has a top-left handle").not.toBeNull();
  expect(h.tl.x, "and it is inside the grid body, not under the row header")
    .toBeGreaterThanOrEqual(s.bodyX0);
  expect(h.tl.y, "nor under the column header").toBeGreaterThanOrEqual(s.bodyY0);
});

test("a whole-column selection offers no range handles", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.touchscreen.tap(box.x + 150, box.y + 120);
  // A column has no on-screen corner to grab — it runs off both ends of the
  // viewport — and Sheets shows no handles for one either.
  await page.evaluate(() => window.opencalcEditor.selectColumn(2, false));
  const h = await page.evaluate(() => window.opencalcEditor.touchHandlesForTest());
  expect(h.tl, "no top-left handle on a column selection").toBeNull();
  expect(h.br, "and none at the bottom-right").toBeNull();
});

// --- The status bar at 390px (UX-MOB-06) ----------------------------------

test("the sheet tabs are visible on a phone, not squeezed to a sliver", async ({ page }) => {
  await boot(page);
  const bar = await page.evaluate(() => {
    const m = (sel) => {
      const el = document.querySelector(sel);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { w: Math.round(r.width), content: el.scrollWidth, right: Math.round(r.right) };
    };
    return {
      docW: document.documentElement.scrollWidth,
      winW: window.innerWidth,
      tabs: m("#sheet-tabs"),
      zoom: m("#zoom-widget"),
    };
  });

  // Measured before the narrow-width layout: 4px wide holding 140px of content.
  // Not "cramped" — there was no visible way on a phone to see which sheet you
  // were on, switch sheet, or add one, because `#sheet-tabs` is the only item in
  // the row that can shrink and three neighbours totalling 340px took the lot.
  // (It clips rather than overdrawing: `.sheet-tabs` is `overflow-x: auto`.)
  expect(bar.tabs.w, `#sheet-tabs is ${bar.tabs.w}px wide`).toBeGreaterThanOrEqual(120);
  expect(bar.tabs.w, `${bar.tabs.content}px of tabs in ${bar.tabs.w}px`)
    .toBeGreaterThanOrEqual(bar.tabs.content);
  // And the room did not come from letting the page slide sideways, which is the
  // failure `UX-EDIT-01` fixed for the toolbar and the reason the zoom controls
  // were left alone last time.
  expect(bar.docW, "the document is no wider than the window").toBeLessThanOrEqual(bar.winW);
  expect(bar.zoom.right, "and the zoom controls are still on screen")
    .toBeLessThanOrEqual(bar.winW);
});
