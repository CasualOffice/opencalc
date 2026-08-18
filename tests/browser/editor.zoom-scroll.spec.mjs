// Scroll-into-view at a magnification other than 100%.
//
// `ensureVisible` mixes two coordinate systems: `getBoundingClientRect` is CSS
// pixels, while the column offsets, the frozen origin and `state.scrollX` are
// grid units, and the canvas is what applies the magnification between them.
// Subtracting one from the other without converting makes the viewport look
// `zoom` times larger than it is (`UX-GRID-02`).
//
// It was fixed without a gate, which is why this exists: the cell still arrives
// on screen at the wrong offset, so nothing user-visible screams, and the whole
// class of bug is invisible to anyone testing at 100%.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// Jump to a cell at `zoom`, and report everything needed to judge the result
/// **without** reusing the calculation under test.
const jumpTo = (page, zoom, row, col) =>
  page.evaluate(
    async ([z, r, c]) => {
      const ed = await import(window.__editorModule);
      ed.setZoomForTest(z);
      ed.selectForTest(r, c);
      const s = ed.scrollStateForTest();
      const w = ed.wasmApi();
      return {
        ...s,
        colLeft: w.session_col_offset_px(0, c),
        colWidth: JSON.parse(w.session_col_px(0, c, 1))[0],
        rowTop: w.session_row_offset_px(0, r),
        rowHeight: JSON.parse(w.session_row_px(0, r, 1))[0],
      };
    },
    [zoom, row, col],
  );

/// The viewport in grid units, derived here from the three raw inputs.
const viewport = (s) => ({ w: s.rectW / s.zoom - s.bodyX0, h: s.rectH / s.zoom - s.bodyY0 });

/// **A cell jumped to at 200% is wholly inside the real viewport.**
///
/// This is the assertion the defect breaks. Believing the viewport is twice as
/// wide as it is, `ensureVisible` scrolls too little, and the cell's trailing
/// edge falls outside the part of the grid actually on screen — while its
/// leading edge is visible, which is why it looks like it worked.
test("a cell scrolled to at 200% fits inside the viewport, not past its edge", async ({ page }) => {
  await boot(page);
  const at = await jumpTo(page, 2, 60, 40);
  const view = viewport(at);

  expect(at.zoom, "the zoom did not take").toBe(2);
  expect(view.w, "the viewport came out non-positive; the harness is wrong, not the editor").toBeGreaterThan(0);

  expect(
    at.colLeft,
    `column 40 starts at ${at.colLeft}, left of scrollX ${at.scrollX}`,
  ).toBeGreaterThanOrEqual(at.scrollX);
  expect(
    at.colLeft + at.colWidth,
    `column 40 ends at ${at.colLeft + at.colWidth}, past the right edge ${at.scrollX + view.w}`,
  ).toBeLessThanOrEqual(at.scrollX + view.w + 0.5);

  expect(at.rowTop).toBeGreaterThanOrEqual(at.scrollY);
  expect(
    at.rowTop + at.rowHeight,
    `row 60 ends at ${at.rowTop + at.rowHeight}, past the bottom edge ${at.scrollY + view.h}`,
  ).toBeLessThanOrEqual(at.scrollY + view.h + 0.5);
});

/// **The same at 50%**, where the error goes the other way.
///
/// Zoomed out the viewport covers *more* grid units, so an unconverted
/// calculation believes it has less room and scrolls further than it needs to.
/// Included because a fix that happened to divide in the wrong direction would
/// satisfy the 200% case alone.
test("a cell scrolled to at 50% also fits, with the error reversed", async ({ page }) => {
  await boot(page);
  const at = await jumpTo(page, 0.5, 60, 40);
  const view = viewport(at);

  expect(at.zoom).toBe(0.5);
  expect(at.colLeft).toBeGreaterThanOrEqual(at.scrollX);
  expect(at.colLeft + at.colWidth).toBeLessThanOrEqual(at.scrollX + view.w + 0.5);
  expect(at.rowTop).toBeGreaterThanOrEqual(at.scrollY);
  expect(at.rowTop + at.rowHeight).toBeLessThanOrEqual(at.scrollY + view.h + 0.5);
});

/// **Zoom changes where you have to scroll to.**
///
/// The control. A calculation that ignored zoom entirely would put the same
/// offset on screen at every magnification and pass both tests above by
/// scrolling too far in a way they cannot see — 200% shows half as many
/// columns, so it must scroll further than 100% to reach the same cell.
test("reaching the same cell needs more scrolling at 200% than at 100%", async ({ page }) => {
  await boot(page);
  const near = await jumpTo(page, 1, 60, 40);
  const far = await jumpTo(page, 2, 60, 40);
  expect(
    far.scrollX,
    `200% scrolled to ${far.scrollX}, no further than 100%'s ${near.scrollX}`,
  ).toBeGreaterThan(near.scrollX);
  expect(far.scrollY).toBeGreaterThan(near.scrollY);
});
