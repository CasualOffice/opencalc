// A merged block keeps its fill and text however it is scrolled.
//
// Reported by the owner with a screen recording (2026-10-17): a report's red
// banners — merges anchored in column A — turned white and lost their titles
// "only while scrolling", as column A left the view. The merge pass took the
// block's fill and text from its top-left cell, looked up among the cells
// fetched for the window; once that cell scrolled out it was not fetched, so the
// block painted with the default background and no text. Excel draws a merged
// cell from its top-left cell wherever the block is on screen.
import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    c.switchSheet(a.session_add_sheet());
    const s = c.state.sheet;
    a.session_set_cell(s, 0, 0, "INTERN MONTHLY REPORT");
    a.session_merge_cells(s, 0, 0, 0, 8);
    a.session_set_fill(s, 0, 0, 0, 8, "C0262D", -1, 0);
    // Content far to the right, so there is somewhere to scroll to.
    a.session_set_cell(s, 0, 40, "far right");
    c.draw();
  }));
}

/// Red pixels across the middle of row 1, and the first drawn column.
const bannerRed = (page) => page.evaluate(() => import("/editor.core.js").then((c) => {
  const cv = document.querySelector("#grid");
  const g = cv.getContext("2d");
  const dpr = window.devicePixelRatio || 1;
  const ri = c.geo.rowIdx.indexOf(0);
  const y = Math.round((c.geo.rowY[ri] + c.geo.rowH[ri] / 2) * dpr);
  const d = g.getImageData(0, y, cv.width, 1).data;
  let red = 0;
  for (let k = 0; k < d.length; k += 4) if (d[k] > 150 && d[k + 1] < 80 && d[k + 2] < 80) red++;
  return { red, firstCol: c.state.firstCol };
}));

test("a merge stays filled while its top-left cell is scrolled out of view", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const seen = [];
  for (let i = 0; i < 40; i += 1) {
    const s = await bannerRed(page);
    seen.push(s);
    if (s.firstCol >= 3) break;
    await page.mouse.wheel(20, 0);
    await page.waitForTimeout(30);
  }
  const outOfView = seen.filter((s) => s.firstCol >= 1);
  expect(outOfView.length, "the scroll must take column A out of view").toBeGreaterThan(0);
  const blank = outOfView.filter((s) => s.red < 50);
  expect(blank, "frames where the banner painted white").toEqual([]);
});

test("its text is still drawn, clipped to the block, from the part on screen", async ({ page }) => {
  await boot(page);
  // Scroll so columns A and B are out (the filled B1 would otherwise be the
  // left-spill fetch that happens to bring A1 in) but the title still shows.
  await page.evaluate(() => import("/editor.core.js").then((c) => { c.state.scrollX = 135; c.draw(); }));
  const dark = await page.evaluate(() => import("/editor.core.js").then((c) => {
    const cv = document.querySelector("#grid");
    const g = cv.getContext("2d");
    const dpr = window.devicePixelRatio || 1;
    const ri = c.geo.rowIdx.indexOf(0);
    const y0 = Math.round(c.geo.rowY[ri] * dpr);
    const h = Math.round(c.geo.rowH[ri] * dpr);
    // Only inside the block: from the grid's left edge to the merge's right edge.
    const x1 = Math.round(c.fscreenXEnd(8) * dpr);
    const d = g.getImageData(0, y0, x1, h).data;
    // The title is drawn over the red fill, in whatever colour contrasts with
    // it: count every pixel inside the block that is neither the fill nor
    // close to it.
    let ink = 0, fill = 0;
    for (let k = 0; k < d.length; k += 4) {
      const [r, gg, b] = [d[k], d[k + 1], d[k + 2]];
      const nearFill = Math.abs(r - 0xc0) < 40 && Math.abs(gg - 0x26) < 40 && Math.abs(b - 0x2d) < 40;
      if (nearFill) fill++;
      // Near-white or near-black only: the block's light-grey outline is neither.
      else if (r > 240 && gg > 240 && b > 240 || r < 60 && gg < 60 && b < 60) ink++;
    }
    // Glyphs over the fill: the block must be mostly fill (an empty white
    // block would count as all "ink"), with some ink on it.
    return { ink, fill, total: d.length / 4, firstCol: c.state.firstCol };
  }));
  expect(dark.firstCol, "columns A and B must both be out of view").toBeGreaterThanOrEqual(2);
  expect(dark.fill, "the block lost its fill").toBeGreaterThan(dark.total / 2);
  expect(dark.ink, "the merged title disappeared").toBeGreaterThan(20);
});
