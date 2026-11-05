// Dragging the fill handle, or dragging out a formula's reference, into the
// edge of the grid scrolls it, as dragging a selection does (PIPE-04) — and as
// Excel does for all three.
//
// Edge auto-scroll was wired to cell and header drags only. A fill drag held at
// the bottom edge sat still, so filling past the last visible row meant
// releasing, scrolling and dragging again; a SUM's range could not be pointed
// further than the window.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "filled");
    a.session_clear_history();
    ed.switchSheet(0);
    ed.renderTabs();
    ed.selectForTest(0, 0);
    ed.draw();
  });
}

const scrollY = (page) => page.evaluate(() => window.opencalcEditor.scrollStateForTest().scrollY);
const geom = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const ed = window.opencalcEditor;
    return { x: ed.colXAt(c), y: ed.rowYAt(r), w: ed.colWAt(c), h: ed.rowHAt(r) };
  }, [r, c]);

test("a fill drag held at the bottom edge scrolls and fills past the window", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const a1 = await geom(page, 0, 0);
  // The fill handle: the selection's bottom-right corner.
  await page.mouse.move(box.x + a1.x + a1.w, box.y + a1.y + a1.h);
  await page.mouse.down();
  await page.mouse.move(box.x + a1.x + a1.w / 2, box.y + box.height / 2, { steps: 4 });
  await page.mouse.move(box.x + a1.x + a1.w / 2, box.y + box.height - 6, { steps: 4 });
  await expect.poll(() => scrollY(page), { timeout: 5_000 }).toBeGreaterThan(400);
  await page.mouse.up();

  // Whatever row the drag ended on is far below the first screenful, and the
  // fill reached it.
  const last = await page.evaluate(() => window.opencalcEditor.selectionRectForTest());
  expect(last.r1).toBeGreaterThan(40);
  const filled = await page.evaluate((r) => window.opencalcEditor.wasmApi().session_cell_input(0, r, 0), last.r1);
  expect(filled).toBe("filled");
  // And the scrolling stopped with the gesture.
  const settled = await scrollY(page);
  await page.waitForTimeout(300);
  expect(await scrollY(page)).toBe(settled);
});

test("a formula's reference drag held at the bottom edge scrolls and extends the range", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 2));
  await page.locator("#grid").focus();
  await page.keyboard.type("=SUM(");
  const box = await page.locator("#grid").boundingBox();
  const b2 = await geom(page, 1, 1);
  await page.mouse.move(box.x + b2.x + b2.w / 2, box.y + b2.y + b2.h / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + b2.x + b2.w / 2, box.y + box.height / 2, { steps: 4 });
  await page.mouse.move(box.x + b2.x + b2.w / 2, box.y + box.height - 6, { steps: 4 });
  await expect.poll(() => scrollY(page), { timeout: 5_000 }).toBeGreaterThan(400);
  await page.mouse.up();

  const text = await page.locator("#formula-input").inputValue();
  const m = /^=SUM\(B2:B(\d+)$/.exec(text);
  expect(m, `the reference should be a B-column range, got ${text}`).not.toBeNull();
  expect(Number(m[1])).toBeGreaterThan(40);
  await page.keyboard.press("Escape");
});
