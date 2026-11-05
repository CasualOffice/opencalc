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

// **A drag inside frozen panes goes to the cell under the pointer.**
//
// Routing the fill and reference drags through the edge-scroll clamp first
// snapped the pointer into the *scrolling* body — below and right of the
// frozen bands — so a fill dragged from A1 to A2 over three frozen rows
// landed on A4 and overwrote A3:A4, and with the body scrolled it filled all
// the way to the first visible body row. The clamp is for a pointer over a
// header or past the edge, not for one over a frozen cell.
async function frozen(page, scrolled) {
  await page.evaluate((scrolled) => import("/editor.core.js").then((c) => {
    const a = c.wasmApi ? c.wasmApi() : window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "head");
    a.session_set_cell(0, 2, 0, "keep2");
    a.session_set_cell(0, 3, 0, "keep3");
    a.session_set_freeze(0, 3, 0);
    a.session_clear_history();
    window.opencalcEditor.selectForTest(0, 0);
    // After the selection, which scrolls its cell into view.
    c.state.scrollX = 0;
    c.state.scrollY = scrolled ? 4000 : 0;
    c.draw();
  }), scrolled);
  if (scrolled) expect(await scrollY(page)).toBe(4000);
}

for (const scrolled of [false, true]) {
  test(`a fill drag inside frozen rows fills to the row under the pointer${scrolled ? ", with the body scrolled" : ""}`, async ({ page }) => {
    await boot(page);
    await frozen(page, scrolled);
    const box = await page.locator("#grid").boundingBox();
    const a1 = await geom(page, 0, 0);
    const a2 = await geom(page, 1, 0);
    await page.mouse.move(box.x + a1.x + a1.w, box.y + a1.y + a1.h);
    await page.mouse.down();
    await page.mouse.move(box.x + a2.x + a2.w / 2, box.y + a2.y + a2.h / 2, { steps: 4 });
    // Long enough for an edge-scroll tick to have run, had one started.
    await page.waitForTimeout(200);
    await page.mouse.up();

    const sel = await page.evaluate(() => window.opencalcEditor.selectionRectForTest());
    const vals = await page.evaluate(() => {
      const a = window.opencalcEditor.wasmApi();
      return [0, 1, 2, 3].map((r) => a.session_cell_input(0, r, 0));
    });
    expect({ r0: sel.r0, r1: sel.r1 }).toEqual({ r0: 0, r1: 1 });
    expect(vals).toEqual(["head", "head", "keep2", "keep3"]);
  });
}

test("a formula's reference drag inside frozen rows names the cells under the pointer", async ({ page }) => {
  await boot(page);
  await frozen(page, true);
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 2));
  await page.locator("#grid").focus();
  await page.keyboard.type("=SUM(");
  await page.evaluate(() => import("/editor.core.js").then((c) => {
    c.state.scrollY = 4000;
    c.draw();
  }));
  expect(await scrollY(page)).toBe(4000);
  const box = await page.locator("#grid").boundingBox();
  const b1 = await geom(page, 0, 1);
  const b2 = await geom(page, 1, 1);
  await page.mouse.move(box.x + b1.x + b1.w / 2, box.y + b1.y + b1.h / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + b2.x + b2.w / 2, box.y + b2.y + b2.h / 2, { steps: 4 });
  await page.waitForTimeout(200);
  await page.mouse.up();
  expect(await page.locator("#formula-input").inputValue()).toBe("=SUM(B1:B2");
  await page.keyboard.press("Escape");
});
