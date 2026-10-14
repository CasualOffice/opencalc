// Cell indicator corners and colours follow Excel (`XL-UX-016`).
//
// Excel draws two kinds of corner mark and never swaps them:
//
//   * a **green triangle in the top-left** — an error-checking flag: a number
//     stored as text, a formula whose result is an error (#DIV/0!, #NAME?, …);
//   * a **red triangle in the top-right** — a note / comment.
//
// OpenCalc had the number-as-text mark green in the top-right (the corner a
// note uses) and a *red* mark top-left on error results, so a cell with an
// error read as "has a comment" at a glance and a number-as-text cell with a
// note showed one triangle hiding the other. The assertions are on the canvas
// pixels: the marks are only ever paint, so the paint is the evidence.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
}

/// The colour at a CSS-pixel point of the grid canvas. The bitmap is `dpr`
/// times the CSS box, and `getImageData` works in bitmap pixels.
const pixel = (page, x, y) =>
  page.evaluate(([x, y]) => {
    const c = document.getElementById("grid");
    const k = c.width / parseFloat(getComputedStyle(c).width);
    const d = c.getContext("2d").getImageData(Math.round(x * k), Math.round(y * k), 1, 1).data;
    return [d[0], d[1], d[2]];
  }, [x, y]);

const isGreen = ([r, g, b]) => g > r + 50 && g > b + 30;
const isRed = ([r, g, b]) => r > g + 80 && r > b + 60;
const kind = (p) => (isGreen(p) ? "green" : isRed(p) ? "red" : `none ${JSON.stringify(p)}`);

/// Sample two pixels just inside each top corner of a cell — the triangle's
/// right angle sits in the corner, so both are inside it whichever size it is.
async function corners(page, row, col) {
  const b = await page.evaluate(([r, c]) => window.opencalcEditor.cellBoxForTest(r, c), [row, col]);
  expect(b, `cell ${row},${col} is on screen`).not.toBeNull();
  const tl = [await pixel(page, b.x + 2, b.y + 2), await pixel(page, b.x + 3, b.y + 2)];
  const tr = [await pixel(page, b.x + b.w - 2, b.y + 2), await pixel(page, b.x + b.w - 3, b.y + 2)];
  const one = (ps) => {
    const ks = ps.map(kind);
    return ks.every((k) => k === ks[0]) ? ks[0] : ks.join("|");
  };
  return { tl: one(tl), tr: one(tr) };
}

test("error flags are green top-left, notes are red top-right", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 1, "=1/0");                // B1  #DIV/0!
    a.session_set_cell(0, 1, 1, "=NOSUCHFN(1)");        // B2  #NAME?
    a.session_set_cell(0, 2, 1, "'10");                 // B3  number stored as text
    a.session_set_comment(0, 3, 1, "a note", "", "");   // B4  note only
    a.session_set_cell(0, 4, 1, "=1/0");                // B5  error + note
    a.session_set_comment(0, 4, 1, "why", "", "");
    a.session_set_cell(0, 5, 1, "'7");                  // B6  text number + note
    a.session_set_comment(0, 5, 1, "and this", "", "");
    a.session_set_cell(0, 6, 1, "42");                  // B7  plain number: no marks
    // Keep the active-cell border well away from every cell sampled.
    window.opencalcEditor.selectForTest(20, 6);
    window.opencalcEditor.draw();
  });
  await page.waitForTimeout(300);

  const got = {};
  for (const [name, r] of [["B1 #DIV/0!", 0], ["B2 #NAME?", 1], ["B3 text-number", 2],
    ["B4 note", 3], ["B5 error+note", 4], ["B6 text-number+note", 5], ["B7 plain", 6]]) {
    got[name] = await corners(page, r, 1);
  }
  expect(got).toEqual({
    "B1 #DIV/0!": { tl: "green", tr: expect.stringMatching(/^none/) },
    "B2 #NAME?": { tl: "green", tr: expect.stringMatching(/^none/) },
    "B3 text-number": { tl: "green", tr: expect.stringMatching(/^none/) },
    "B4 note": { tl: expect.stringMatching(/^none/), tr: "red" },
    "B5 error+note": { tl: "green", tr: "red" },
    "B6 text-number+note": { tl: "green", tr: "red" },
    "B7 plain": { tl: expect.stringMatching(/^none/), tr: expect.stringMatching(/^none/) },
  });
});

/// The hover explanation keys on the cell, not the corner, so moving the mark
/// must not lose it: hovering an error still explains the error.
test("hovering a flagged cell still explains it", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 1, "=1/0");
    a.session_set_cell(0, 2, 1, "'10");
    window.opencalcEditor.selectForTest(20, 6);
    window.opencalcEditor.draw();
  });
  await page.waitForTimeout(300);
  const box = await page.locator("#grid").boundingBox();
  for (const [r, want] of [[0, /#DIV\/0!/], [2, /Number stored as text/]]) {
    const b = await page.evaluate((r) => window.opencalcEditor.cellBoxForTest(r, 1), r);
    // Over the top-left mark itself, where a user aims.
    await page.mouse.move(box.x + b.x + 3, box.y + b.y + 3);
    await page.mouse.move(box.x + b.x + 4, box.y + b.y + 4);
    await page.waitForTimeout(150);
    const tip = await page.evaluate(() => {
      const t = document.getElementById("comment-tip");
      return t && !t.hidden ? t.textContent : "";
    });
    expect(tip).toMatch(want);
  }
});
