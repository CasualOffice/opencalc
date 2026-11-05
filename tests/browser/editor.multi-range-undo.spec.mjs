// One command over a Ctrl+click selection is one undo step (PIPE-02), as in
// Excel.
//
// Each range of the selection is its own engine call and so was its own
// history entry: Bold over three ranges took three Ctrl+Zs to take back, and
// the first two left the selection half-bold. The same held for Clear Formats,
// Clear All and Delete.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    for (const [r, c] of [[1, 1], [3, 3], [5, 5]]) a.session_set_cell(0, r, c, `v${r}`);
    a.session_clear_history();
    ed.switchSheet(0);
    ed.renderTabs();
    ed.draw();
  });
}

const centre = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const ed = window.opencalcEditor;
    return { x: ed.colXAt(c) + ed.colWAt(c) / 2, y: ed.rowYAt(r) + ed.rowHAt(r) / 2 };
  }, [r, c]);

/// B2, then Ctrl+click D4 and F6: three ranges.
async function selectThree(page) {
  const box = await page.locator("#grid").boundingBox();
  const at = async (r, c) => { const p = await centre(page, r, c); return [box.x + p.x, box.y + p.y]; };
  await page.mouse.click(...(await at(1, 1)));
  await page.keyboard.down("Control");
  await page.mouse.click(...(await at(3, 3)));
  await page.mouse.click(...(await at(5, 5)));
  await page.keyboard.up("Control");
}

const bold = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [[1, 1], [3, 3], [5, 5]].map(([r, c]) => JSON.parse(a.session_cell_format(0, r, c)).b === 1);
  });
const inputs = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [[1, 1], [3, 3], [5, 5]].map(([r, c]) => a.session_cell_input(0, r, c));
  });
const undo = (page) => page.evaluate(() => window.opencalcEditor.wasmApi().session_undo());
const canUndo = (page) => page.evaluate(() => window.opencalcEditor.wasmApi().session_can_undo());

test("Bold over three Ctrl+click ranges is taken back by one Ctrl+Z", async ({ page }) => {
  await boot(page);
  await selectThree(page);
  await page.keyboard.press("Control+b");
  expect(await bold(page)).toEqual([true, true, true]);
  await page.keyboard.press("Control+z");
  expect(await bold(page), "one undo takes back all three").toEqual([false, false, false]);
  expect(await canUndo(page), "and nothing is left over").toBe(false);
});

test("Delete over three ranges is one undo step", async ({ page }) => {
  await boot(page);
  await selectThree(page);
  await page.keyboard.press("Delete");
  expect(await inputs(page)).toEqual(["", "", ""]);
  await undo(page);
  expect(await inputs(page)).toEqual(["v1", "v3", "v5"]);
  expect(await canUndo(page)).toBe(false);
});

test("Clear Formats and Clear All over three ranges are one undo step each", async ({ page }) => {
  await boot(page);
  await selectThree(page);
  await page.keyboard.press("Control+b");
  await page.evaluate(() => window.opencalcEditor.clearFormats());
  expect(await bold(page)).toEqual([false, false, false]);
  await undo(page);
  expect(await bold(page), "Clear Formats undone in one").toEqual([true, true, true]);

  await page.evaluate(() => window.opencalcEditor.clearAll());
  expect(await inputs(page)).toEqual(["", "", ""]);
  await undo(page);
  expect(await inputs(page), "Clear All undone in one").toEqual(["v1", "v3", "v5"]);
  expect(await bold(page)).toEqual([true, true, true]);
  // What remains is the Bold, as one step too.
  await undo(page);
  expect(await bold(page)).toEqual([false, false, false]);
  expect(await canUndo(page)).toBe(false);
});
