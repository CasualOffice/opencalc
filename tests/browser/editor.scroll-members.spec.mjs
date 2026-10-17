// The members of the scroll-invariance class (`XL-UX-183`) that the property
// test in `editor.scroll-invariance.spec.mjs` found, one focused test each.
//
// Each test draws one cell twice: once with the cell that decides its look on
// screen, once with that cell scrolled out — and requires the same picture.
// Excel (Microsoft 365): what a cell shows never depends on scrolling.
import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

/// A sheet of its own, seeded by `fn(wasm, sheet)`.
async function seed(page, body) {
  await page.evaluate((body) => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    c.switchSheet(a.session_add_sheet());
    const s = c.state.sheet;
    // eslint-disable-next-line no-new-func
    new Function("a", "s", body)(a, s);
    a.session_set_cell(s, 150, 60, "far"); // somewhere to scroll to
    c.selectForTest(140, 55);
    c.draw();
  }), body);
  // The theme's colours ease in after load; wait for a still frame.
  await expect.poll(() => page.evaluate(() => import("/editor.core.js").then(async (c) => {
    const cv = document.querySelector("#grid");
    const g = cv.getContext("2d");
    c.state.scrollX = 0; c.state.scrollY = 0; c.draw();
    const x = g.getImageData(0, 0, cv.width, cv.height).data;
    await new Promise((r) => setTimeout(r, 150));
    c.draw();
    const y = g.getImageData(0, 0, cv.width, cv.height).data;
    for (let k = 0; k < x.length; k += 1) if (x[k] !== y[k]) return false;
    return true;
  })), { timeout: 10_000 }).toBe(true);
}

/// The RGB of cell (r, c) drawn at scroll (sx, sy), plus where it landed.
const cellAt = (page, r, col, sx, sy) => page.evaluate(([r, col, sx, sy]) => import("/editor.core.js").then((c) => {
  c.state.scrollX = sx; c.state.scrollY = sy;
  c.draw();
  const ci = c.geo.colIdx.indexOf(col), ri = c.geo.rowIdx.indexOf(r);
  const x = c.geo.colX[ci], y = c.geo.rowY[ri], w = c.geo.colW[ci], h = c.geo.rowH[ri];
  const d = document.querySelector("#grid").getContext("2d").getImageData(x, y, w, h).data;
  const rgb = [];
  for (let k = 0; k < d.length; k += 4) rgb.push(d[k], d[k + 1], d[k + 2]);
  return { x, y, w, h, rgb, firstCol: c.state.firstCol, firstRow: c.state.firstRow };
}), [r, col, sx, sy]);

/// How many channels differ by more than a seam's worth (see the property
/// test for the one rasteriser artefact this allows for).
function differing(a, b) {
  expect([a.w, a.h]).toEqual([b.w, b.h]);
  let n = 0;
  for (let k = 0; k < a.rgb.length; k += 1) if (Math.abs(a.rgb[k] - b.rgb[k]) > 12) n += 1;
  return n;
}

test("a long label does not spill through an empty merged block that has scrolled out", async ({ page }) => {
  // Within the window, an empty merge blocks a neighbour's overflow (the
  // `occupied` set marks a merge's whole span). Off the window it did not: the
  // set holds only the window's columns, so a label in A22 whose path crossed
  // an empty merge in D22:E22 spilled straight through it into G22 the moment
  // D and E scrolled out.
  await boot(page);
  await seed(page, `
    a.session_set_cell(s, 21, 0, "A long label in A22 that an empty merge in D22:E22 has to stop");
    a.session_merge_cells(s, 21, 3, 21, 4);
  `);
  const inView = await cellAt(page, 21, 6, 0, 0);
  const merged = await cellAt(page, 21, 6, 320, 0);
  expect(merged.firstCol, "D and E must be out of view").toBe(5);
  expect(differing(inView, merged), "G22 showed text that the merge stops on screen").toBe(0);
});

test("a border on the edge shared with a cell that has scrolled out is still drawn", async ({ page }) => {
  // A border wider than a pixel is centred on its edge, so half of a cell's
  // thick right border lies in the cell to its right. The border pass drew
  // only the window's cells, so once the bordered cell scrolled out exactly to
  // that edge, its neighbour lost the half that sat inside it: a green rule
  // along G31's left side thinned, and a purple rule along the top of row 31,
  // depending on nothing but where the view stopped.
  await boot(page);
  await seed(page, `
    a.session_set_border(s, 30, 5, 30, 5, "right", "thick", "00B050");
    a.session_set_border(s, 29, 2, 29, 9, "bottom", "medium", "7030A0");
  `);
  // Column F out of view, column G exactly at the left edge.
  const before = await cellAt(page, 30, 6, 0, 400);
  const after = await cellAt(page, 30, 6, 384, 400);
  expect(after.firstCol, "G must be the first column, at the edge").toBe(6);
  expect(differing(before, after), "G31 lost the half of F31's border that lies inside it").toBe(0);
  // Row 30 out of view, row 31 exactly at the top edge.
  const top0 = await cellAt(page, 30, 4, 0, 400);
  const top1 = await cellAt(page, 30, 4, 0, 600);
  expect(top1.firstRow, "row 31 must be the first row, at the edge").toBe(30);
  expect(differing(top0, top1), "E31 lost the half of E30's bottom border that lies inside it").toBe(0);
});
