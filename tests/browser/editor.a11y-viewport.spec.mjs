// The accessibility mirror has to describe the screen that is there (`A11Y-01`).
//
// The grid is drawn on a canvas, so `#grid-a11y` is not a convenience for a
// screen reader — it *is* the grid, the only representation of it that reaches
// the accessibility tree at all. A mirror describing rows the view left behind
// is therefore not a stale detail; it is a user being read a screen that does
// not exist, with no way to tell.
//
// **What the original measurement got wrong, and why it is repeated here.**
// `A11Y-01` and `docs/12` §4.6 both report "after scrolling to scrollY 3360 the
// mirror's first `aria-rowindex` is still 1". Taken literally that is
// `document.querySelector("[aria-rowindex]")`, which is the mirror's
// **column-header row** — `rebuildA11yGrid` gives it `aria-rowindex="1"` on
// purpose, and it is 1 at every scroll position by construction. So the number
// that was quoted proves nothing either way, and the settled mirror was in fact
// correct all along. These tests read the first row that contains a `gridcell`,
// which is the first *data* row, and compare it against `geo.rowIdx[0]` — the
// row the renderer just drew at the top of the view.
//
// The defect underneath the bad measurement is real and is about *when*:
// the rebuild was deferred with a settle timer that every scroll frame cleared
// and re-armed, so while the view kept moving the mirror was never rebuilt at
// all. Its staleness was bounded by the length of the gesture and by nothing
// else — measured at **220 rows behind the screen after 1.5s of continuous
// scrolling**, and unbounded in principle.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    // A fresh session first: the sample workbook carries its own styles, and a
    // probe that writes over it is measuring the sample.
    window.opencalcEditor.wasmApi().session_new();
    const line = Array.from({ length: 12 }, (_, c) => (c % 3 ? "12345" : "Item text")).join("\t");
    const tsv = Array.from({ length: 800 }, () => line).join("\n");
    window.opencalcEditor.wasmApi().session_paste_tsv(0, 0, 0, tsv);
  });
  await page.waitForTimeout(400);
}

/// What the mirror says the top and bottom of the screen are, against what the
/// renderer actually drew there.
const view = (page) => page.evaluate(() => {
  const ed = window.opencalcEditor;
  const rows = [...document.querySelectorAll('#grid-a11y [role="row"]')]
    .filter((r) => r.querySelector('[role="gridcell"]'));
  // `aria-rowindex` is 1-based *and* offset by the header row the mirror puts
  // at index 1, which is the `+2` `rebuildA11yGrid` writes.
  const idx = (el) => (el ? Number(el.getAttribute("aria-rowindex")) - 2 : null);
  return {
    firstVisibleRow: ed.geo.rowIdx[0],
    lastVisibleRow: ed.geo.rowIdx[ed.geo.rowIdx.length - 1],
    mirrorFirstRow: idx(rows[0]),
    mirrorLastRow: idx(rows[rows.length - 1]),
  };
});

test("the mirror follows the view while the view is still moving", async ({ page }) => {
  await boot(page);
  const ceiling = await page.evaluate(() => window.opencalcEditor.a11yMaxStaleMsForTest());
  // The tolerance below is expressed in terms of the ceiling so the two cannot
  // drift apart — which also means the ceiling itself has to be pinned, or
  // raising it would relax this test instead of failing it. A quarter second is
  // the shipped value and 400ms is the outer edge of "a reader cannot finish
  // reading a row that has already left the screen".
  expect(ceiling, "the staleness ceiling is part of the contract, not a tuning knob")
    .toBeLessThanOrEqual(400);

  // Sample the mirror against the view on every frame of a long, unbroken
  // scroll — long enough that a settle-only mirror never gets a chance to
  // catch up, which is exactly the case the defect lived in.
  const trace = await page.evaluate(async () => {
    const ed = window.opencalcEditor;
    const grid = document.querySelector("#grid");
    const samples = [];
    const t0 = performance.now();
    while (performance.now() - t0 < 1500) {
      grid.dispatchEvent(new WheelEvent("wheel", { deltaY: 60, deltaMode: 0, bubbles: true, cancelable: true }));
      await new Promise((r) => requestAnimationFrame(r));
      const rows = [...document.querySelectorAll('#grid-a11y [role="row"]')]
        .filter((r) => r.querySelector('[role="gridcell"]'));
      samples.push({
        t: performance.now() - t0,
        view: ed.geo.rowIdx[0],
        mirror: rows[0] ? Number(rows[0].getAttribute("aria-rowindex")) - 2 : null,
      });
    }
    return samples;
  });

  const travelled = trace[trace.length - 1].view - trace[0].view;
  expect(travelled, "the view did not move, so this proves nothing").toBeGreaterThan(120);

  // How out of date the mirror was, in milliseconds rather than rows: the row
  // count depends on how fast this machine happened to scroll, and the contract
  // is a time. The scroll is monotonic, so "when was the mirror last true" is
  // the last moment the top of the view had not yet passed the row the mirror
  // is showing.
  let worst = { staleMs: 0 };
  for (const s of trace) {
    if (s.mirror === null) continue;
    let lastTrue = trace[0].t;
    for (const p of trace) {
      if (p.t > s.t) break;
      if (p.view <= s.mirror) lastTrue = p.t;
    }
    const staleMs = s.t - lastTrue;
    if (staleMs > worst.staleMs) worst = { staleMs, at: Math.round(s.t), view: s.view, mirror: s.mirror };
  }

  // One frame of slack over the ceiling for the frame the rebuild is scheduled
  // from, and one more for the task it runs in.
  expect(
    worst.staleMs,
    `the mirror was ${Math.round(worst.staleMs)}ms out of date at t=${worst.at}ms `
      + `(showing row ${worst.mirror}, view at row ${worst.view}) — the ceiling is ${ceiling}ms`,
  ).toBeLessThan(ceiling + 150);
});

test("the mirror is exact once the scroll settles", async ({ page }) => {
  await boot(page);
  await page.evaluate(async () => {
    const grid = document.querySelector("#grid");
    for (let i = 0; i < 40; i += 1) {
      grid.dispatchEvent(new WheelEvent("wheel", { deltaY: 100, deltaMode: 0, bubbles: true, cancelable: true }));
      await new Promise((r) => requestAnimationFrame(r));
    }
  });
  // Distinct from the assertion above on purpose. Bounding the staleness *while*
  // moving would be satisfied by a mirror that is permanently a quarter second
  // behind; what has to be true when the motion stops is not "close enough" but
  // "the same rows", and that is what a reader navigating a settled screen gets.
  await expect.poll(async () => (await view(page)).mirrorFirstRow, { timeout: 3000 })
    .toBe((await view(page)).firstVisibleRow);
  const v = await view(page);
  expect(v.firstVisibleRow, "the view did not move, so this proves nothing").toBeGreaterThan(100);
  expect(v.mirrorFirstRow, "the settled mirror starts where the screen starts").toBe(v.firstVisibleRow);
  expect(v.mirrorLastRow, "and ends where the screen ends").toBe(v.lastVisibleRow);
});

test("the header row's aria-rowindex is 1 at every scroll position", async ({ page }) => {
  // The measurement that produced `A11Y-01`'s number, pinned as the constant it
  // is. Without this, the next person to read `[aria-rowindex]` off the document
  // and find a 1 re-files the same bug.
  await boot(page);
  const headerIndex = () => page.evaluate(() =>
    document.querySelector("#grid-a11y [role='row']").getAttribute("aria-rowindex"));
  expect(await headerIndex()).toBe("1");
  await page.evaluate(() => window.opencalcEditor.selectForTest(200, 0));
  await page.waitForTimeout(300);
  const v = await view(page);
  expect(v.firstVisibleRow, "the selection did not scroll the view").toBeGreaterThan(100);
  expect(await headerIndex(), "the column-header row is grid row 1 wherever the view is").toBe("1");
  expect(v.mirrorFirstRow, "and the first *data* row is the one that moves").toBe(v.firstVisibleRow);
});
