// Excel's End mode: press End, then an arrow, and the cursor jumps to the edge
// of the data block.
//
// It is how a keyboard user crosses a large sheet without holding a modifier,
// and it was the one navigation idiom missing. `End` on its own used to jump to
// the last used column — which is Excel's *End then Right* — so the destination
// existed while the mode it belongs to did not, and the two-key sequence a
// spreadsheet user has in their fingers did nothing at all (`UX-END-01`).

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
  // A block of data with a gap after it, so "the edge of the block" and "the
  // next cell" are different answers and a test can tell them apart.
  await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    for (let c = 0; c < 5; c += 1) w.session_set_cell(0, 0, c, String(c + 1));
    for (let r = 0; r < 5; r += 1) w.session_set_cell(0, r, 0, String(r + 1));
    ed.selectForTest(0, 0);
  });
  await page.locator("#grid").click({ position: { x: 120, y: 60 } });
  await page.evaluate(async () => (await import(window.__editorModule)).selectForTest(0, 0));
}

const at = (page) =>
  page.evaluate(async () => {
    const s = (await import(window.__editorModule)).scrollStateForTest();
    return { row: s.row, col: s.col };
  });

/// **End alone arms the mode and moves nothing.**
///
/// This is the behaviour change: it used to jump to the last used column.
test("End on its own arms the mode without moving the cursor", async ({ page }) => {
  await boot(page);
  const before = await at(page);
  await page.keyboard.press("End");
  expect(await at(page), "End moved the cursor instead of arming the mode").toEqual(before);
  await expect(page.locator("#tb-status")).toHaveText(/End mode/i);
});

/// **End then an arrow jumps to the edge of the block.**
test("End then ArrowRight jumps to the end of the data block", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("End");
  await page.keyboard.press("ArrowRight");
  // Row 0 holds five cells, so the block edge is column 4 — not column 1, which
  // is where a plain arrow would land.
  expect(await at(page)).toEqual({ row: 0, col: 4 });
});

/// **End then Home is the last used cell**, as in Excel.
test("End then Home goes to the last used cell", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("End");
  await page.keyboard.press("Home");
  const where = await at(page);
  expect(where.row).toBe(4);
  expect(where.col).toBe(4);
});

/// **The mode is spent, not sticky.**
///
/// An armed mode that survives its own use fires again on an arrow the user has
/// long forgotten arming.
test("the mode is spent by the arrow that used it", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("End");
  await page.keyboard.press("ArrowRight");
  expect(await at(page)).toEqual({ row: 0, col: 4 });
  // A second arrow is an ordinary step, not another jump.
  await page.keyboard.press("ArrowLeft");
  expect(await at(page), "the second arrow jumped as well — the mode was not spent").toEqual({ row: 0, col: 3 });
});

/// **A plain arrow still moves one cell.** The control: a jump on every arrow
/// would satisfy the tests above and destroy ordinary navigation.
test("an arrow with no End before it moves a single cell", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("ArrowRight");
  expect(await at(page)).toEqual({ row: 0, col: 1 });
});

/// **Escape disarms it**, so an armed mode is escapable like everything else.
test("Escape disarms an armed End mode", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("End");
  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowRight");
  expect(await at(page), "the arrow still jumped after Escape").toEqual({ row: 0, col: 1 });
});
