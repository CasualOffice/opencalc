// Scrolling up and down must not drift sideways.
//
// Reported from the desktop app: "while scrolling up and down it scrolled
// automatically slightly right, I tried this multiple times."
//
// A trackpad does not deliver a pure axis. A two-finger vertical scroll carries
// a small `deltaX` from the hand's own drift — a pixel or two per event, tens of
// events per second — and the wheel handler added every one of them to
// `scrollX`. The vertical component is what the user is watching, so the
// horizontal creep is invisible until the sheet is somewhere they did not put
// it. It accumulates rather than cancelling, because a hand drifts one way.
//
// Native applications lock to the dominant axis for exactly this reason.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  // Something to scroll through. `clampScroll` holds the view to the used
  // range, so on a blank sheet every one of these assertions would pass or fail
  // for the wrong reason — there is nowhere to go.
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 200; r += 1) {
      for (const c of [0, 20, 40, 60]) a.session_set_cell(0, r, c, `r${r}c${c}`);
    }
  });
  await page.waitForTimeout(250);
}

const scroll = (page) => page.evaluate(() => window.opencalcEditor.scrollStateForTest());

/** A burst of wheel events, the way a trackpad delivers a gesture. */
async function wheel(page, events) {
  await page.evaluate(async (events) => {
    const grid = document.querySelector("#grid");
    for (const [dx, dy] of events) {
      grid.dispatchEvent(
        new WheelEvent("wheel", { deltaX: dx, deltaY: dy, deltaMode: 0, bubbles: true, cancelable: true }),
      );
      await new Promise((r) => requestAnimationFrame(r));
    }
  }, events);
  await page.waitForTimeout(120);
}

test("a vertical gesture with trackpad noise does not creep sideways", async ({ page }) => {
  await boot(page);
  // 40 events of honest vertical scroll, each carrying the 1-2px of horizontal
  // noise a real trackpad produces. Down, then back up.
  const down = Array.from({ length: 40 }, (_, i) => [i % 3 === 0 ? 2 : 1, 40]);
  const up = Array.from({ length: 40 }, (_, i) => [i % 3 === 0 ? 2 : 1, -40]);
  await wheel(page, [...down, ...up]);

  const after = await scroll(page);
  // Back where it started vertically, and — the point — still at the left edge.
  expect(after.scrollX, "scrolling up and down must not move sideways").toBe(0);
});

test("a deliberate sideways gesture still scrolls sideways", async ({ page }) => {
  await boot(page);
  // The fix must not be "ignore deltaX": a genuine horizontal swipe is how you
  // reach column BF, and a trackpad is the only way to make one.
  await wheel(page, Array.from({ length: 20 }, () => [40, 1]));
  expect((await scroll(page)).scrollX, "a horizontal swipe still works").toBeGreaterThan(100);
});

test("a diagonal gesture still moves on both axes", async ({ page }) => {
  await boot(page);
  // Genuinely diagonal — neither axis dominant. Locking to one would make the
  // grid fight the hand.
  await wheel(page, Array.from({ length: 20 }, () => [30, 30]));
  const after = await scroll(page);
  expect(after.scrollX, "diagonal keeps its horizontal component").toBeGreaterThan(50);
  expect(after.scrollY, "and its vertical one").toBeGreaterThan(50);
});
