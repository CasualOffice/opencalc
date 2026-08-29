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
//
// The mirror's half of that is now a **rate**, not a zero (`A11Y-01`): it is
// rebuilt at most once per `A11Y_MAX_STALE_MS` while the view moves, because a
// mirror that waits for the scroll to stop describes rows that are not on the
// screen for the whole length of the gesture. Measured cost of the change, with
// `frame-profile.mjs` over 1.5s of continuous scrolling: median frame 16.7ms
// before and after; on the widest window (41 columns, 1200 mirrored cells) the
// tail moves from max 17.7ms / 0-1 frames over 20ms, to max ~25ms / ~5 frames
// over 20ms — about four frames a second paying ~5ms of DOM rebuild. On an
// ordinary 13-column window the cost is at the edge of measurable.

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

test("scrolling does not rebuild the accessibility mirror per frame", async ({ page }) => {
  await boot(page);
  await page.waitForTimeout(300); // let any settle-timer from boot fire
  const before = await rebuilds(page);
  const t0 = Date.now();
  await scrollBurst(page, 40);
  const elapsed = Date.now() - t0;
  const ceiling = await page.evaluate(() => window.opencalcEditor.a11yMaxStaleMsForTest());
  const n = await rebuilds(page) - before;

  // This asserted `toBe(0)` until `A11Y-01`, and zero turned out to be the
  // wrong number rather than a strict one: a mirror that is never rebuilt while
  // the view moves is a mirror that describes rows that are not on screen, for
  // as long as the gesture lasts, and the canvas has no other accessible
  // representation. `A11Y_MAX_STALE_MS` is the ceiling that replaced it.
  //
  // What this gate exists for survives intact, because the thing that was
  // actually wrong was *per-frame* work: the rebuild ran on all 40 of these
  // frames, dirtying layout on each. The bound below is a rate, and the two
  // halves say different things — the ceiling arithmetic catches the ceiling
  // being quietly lowered, and the fraction catches the rate creeping back
  // toward one-per-frame however the ceiling is spelled.
  expect(n, `${n} rebuilds in ${elapsed}ms of scrolling, ceiling ${ceiling}ms`)
    .toBeLessThanOrEqual(Math.ceil(elapsed / ceiling) + 1);
  expect(n, `${n} rebuilds over 40 scroll frames — the mirror is not frame work`)
    .toBeLessThanOrEqual(10);
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

  // Still not asserted here, deliberately: *which* rows the mirror describes.
  // That is `A11Y-01` and it now has its own gate in
  // `editor.a11y-viewport.spec.mjs`. Keeping the two apart is the point — this
  // one has to be able to fail for a frame-budget regression and nothing else.
  //
  // (The observation this comment used to carry, that "after scrolling 1600px
  // its first `aria-rowindex` is still 1", was a bad measurement: that element
  // is the mirror's column-header row, which is grid row 1 at every scroll
  // position by construction. The defect underneath it was real; the number
  // quoted for it was not.)
});
