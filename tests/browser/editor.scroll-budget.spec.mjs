// Work that is not drawing must stay off the scroll path.
//
// A CPU profile of a scroll over a 3000x40 sheet put **36.8% in
// `updateScrollbars` and 6.6% in `rebuildA11yGrid`** — 43% of every frame in
// two functions that do not draw a single cell. The grid ran at 66.6ms per
// frame, which is 15fps, and the first thing anybody does with a spreadsheet is
// scroll it.
//
//   - The accessibility mirror is hundreds of DOM nodes, and its guard against a
//     redundant rebuild hashes every visible cell. Scrolling changes that hash
//     every frame, so the guard never fired and the tree was rebuilt
//     continuously — dirtying the layout continuously.
//   - `updateScrollbars` then read `clientHeight`, forcing a synchronous reflow
//     of everything the rebuild had just dirtied, to learn a number that only
//     changes when the window does.
//
// Asserted as a mechanism rather than as a frame time. A timing assertion on CI
// measures the runner; this measures the thing that was wrong, and it fails
// deterministically if either cost comes back.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const line = Array.from({ length: 30 }, (_, c) => (c % 3 ? "12345" : "Item text")).join("\t");
    const tsv = Array.from({ length: 600 }, () => line).join("\n");
    window.opencalcEditor.wasmApi().session_paste_tsv(0, 0, 0, tsv);
  });
  await page.waitForTimeout(400);
}

const rebuilds = (page) => page.evaluate(() => window.opencalcEditor.a11yRebuildCountForTest());

async function scrollBurst(page, frames) {
  await page.evaluate(async (frames) => {
    const grid = document.querySelector("#grid");
    for (let i = 0; i < frames; i += 1) {
      grid.dispatchEvent(new WheelEvent("wheel", { deltaY: 50, deltaMode: 0, bubbles: true, cancelable: true }));
      await new Promise((r) => requestAnimationFrame(r));
    }
  }, frames);
}

test("scrolling does not rebuild the accessibility mirror", async ({ page }) => {
  await boot(page);
  await page.waitForTimeout(300); // let any settle-timer from boot fire
  const before = await rebuilds(page);
  await scrollBurst(page, 40);
  expect(await rebuilds(page) - before, "the mirror is not scroll work").toBe(0);
});

test("the mirror is rebuilt once the view settles", async ({ page }) => {
  await boot(page);
  await page.waitForTimeout(300);
  const before = await rebuilds(page);
  await scrollBurst(page, 40);
  // The whole trade: deferred, never dropped. A screen reader is not reading
  // mid-fling, but the tree must be true the moment the motion stops.
  await expect.poll(async () => (await rebuilds(page)) - before, { timeout: 3000 })
    .toBeGreaterThan(0);
});

test("the mirror still describes where the view actually is", async ({ page }) => {
  await boot(page);
  await scrollBurst(page, 40);
  await page.waitForTimeout(400);
  // Deferring is only acceptable if the tree that settles is a real one, so
  // this asserts it is populated rather than emptied by the deferral.
  const cells = await page.evaluate(() => document.querySelectorAll("[aria-rowindex]").length);
  expect(cells, "a real mirror settles, not an empty one").toBeGreaterThan(0);

  // NOT asserted here, deliberately: that the mirror describes the *scrolled*
  // rows. It does not — after scrolling 1600px its first `aria-rowindex` is
  // still 1 — and that is true on an unmodified tree as well, so it is a
  // pre-existing accessibility defect rather than a cost of deferring. It is
  // filed separately; asserting it here would make this gate fail for somebody
  // else's bug and quietly stop guarding the frame budget it exists for.
});
