// The editor canvas and the headless renderer must draw a data bar the same.
//
// There is no display list across the WebAssembly boundary, so the canvas paints
// its own bars. That is fine; what was not is that it also *decided* their
// geometry — the inset, the alpha and the default colour were written out twice,
// in `casual-calc-render` and in `editor.js`, and agreed only because somebody
// had copied them across (`RND-08`).
//
// The engine now exports them. This asserts the canvas follows that export
// rather than its own copy, by measuring what was actually painted: a test that
// compared the export against itself would pass with the canvas hardcoding
// whatever it liked.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// Put a full-length data bar in A1 and measure the painted result.
const paintAndMeasure = (page) =>
  page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    // 1 and 100 so A2 is the range maximum and draws the longest bar there is.
    w.session_set_cell(0, 0, 0, "1");
    w.session_set_cell(0, 1, 0, "100");
    w.session_add_cf(0, 0, 0, 1, 0, "databar", 0, 0, "FF0000", "");
    ed.selectForTest(0, 0);
    // Two frames, so the paint that follows the edit has certainly happened.
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

    const style = JSON.parse(w.session_data_bar_style());
    const canvas = document.getElementById("grid");
    const ctx = canvas.getContext("2d");
    const dpr = canvas.width / canvas.getBoundingClientRect().width;

    // The header sizes, from the editor's own view of its layout: with nothing
    // frozen, the body origin *is* the header. Guessing them would be hunting a
    // few pixels and would go stale the first time a header default changed.
    const view = ed.scrollStateForTest();
    const headerW = view.bodyX0;
    const headerH = view.bodyY0;
    const colW = JSON.parse(w.session_col_px(0, 0, 1))[0];
    const rowH = JSON.parse(w.session_row_px(0, 0, 1))[0];

    // A2 holds the range maximum, so its bar is the longest one drawn.
    const y = Math.round((headerH + rowH + rowH / 2) * dpr);
    const red = (x) => {
      const d = ctx.getImageData(Math.round(x * dpr), y, 1, 1).data;
      return d[0] > 200 && d[1] < 220 && d[2] < 220;
    };

    let first = null;
    let last = null;
    for (let x = headerW; x < headerW + colW; x += 1) {
      if (red(x)) {
        if (first === null) first = x;
        last = x;
      }
    }
    return { style, first, last, headerW, colW, dpr };
  });

/// **The bar starts at the inset the engine names, not one the canvas chose.**
test("the canvas insets a data bar by the engine's padX", async ({ page }) => {
  await boot(page);
  const at = await paintAndMeasure(page);

  expect(at.first, "no data bar was painted at all").not.toBeNull();
  const inset = at.first - at.headerW;
  expect(
    Math.abs(inset - at.style.padX),
    `the bar starts ${inset}px into the cell; the engine says ${at.style.padX}px`,
  ).toBeLessThanOrEqual(1);
});

/// **And ends at the far inset**, which is what a wrong padX moves most.
test("the canvas ends a full-length data bar at the engine's padX", async ({ page }) => {
  await boot(page);
  const at = await paintAndMeasure(page);

  expect(at.last).not.toBeNull();
  // A full-length bar runs to `colW - padX`; the fraction for the range maximum
  // is ECMA-376's `maxLength`, so the engine's own arithmetic decides the rest —
  // this only checks the canvas did not invent its own inset.
  const rightInset = at.headerW + at.colW - at.last;
  expect(
    rightInset,
    `the bar ends ${rightInset}px from the cell's right edge`,
  ).toBeGreaterThanOrEqual(at.style.padX);
});

/// **The exported style is what the canvas can actually use.**
///
/// A control: the values have to be numbers and a colour, not strings or
/// `undefined` that would silently make every `fillRect` a no-op.
test("the engine exports a usable data bar style", async ({ page }) => {
  await boot(page);
  const style = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    return JSON.parse(ed.wasmApi().session_data_bar_style());
  });
  expect(typeof style.padX).toBe("number");
  expect(typeof style.padY).toBe("number");
  expect(style.alpha).toBeGreaterThan(0);
  expect(style.alpha).toBeLessThanOrEqual(1);
  expect(style.defaultColor).toMatch(/^[0-9A-Fa-f]{6}$/);
});
