// Resizing a column has to reflow its contents *during* the drag.
//
// Reported from a running editor: "expanding width of column and row height ..
// its not realtime .. untill i leave it .. than content rearrange .. not
// fluidly". The client already previewed the *geometry*, so the column edge
// moved — but the display text comes from the engine, and the engine still held
// the old width until the mouse came up. The edge slid out from under
// stationary text and everything snapped at the end.

import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

/// The engine's own width for a column, in device pixels.
const widthOf = (page, col) =>
  page.evaluate((c) => window.opencalcEditor.wasmApi().session_col_width(0, c), col);

const canUndo = (page) =>
  page.evaluate(() => window.opencalcEditor.wasmApi().session_can_undo());

test("a column reflows while it is being dragged, not when it is released", async ({ page }) => {
  const problems = await boot(page);

  const before = await widthOf(page, 1);
  const undoBefore = await canUndo(page);

  // The column header's right edge, which is where a resize is grabbed.
  const edge = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    return { x: ed.HW + ed.colXAt(1) - ed.HW + ed.colWAt(1), y: ed.HH / 2 };
  }).catch(() => null);

  const box = await page.locator("#grid").boundingBox();
  // Fall back to a measured offset if the internals are not exposed: the point
  // only has to land on the boundary between column B and C.
  const px = edge ? edge.x : 64 * 2 + 40;
  const py = edge ? edge.y : 10;

  await page.mouse.move(box.x + px, box.y + py);
  await page.mouse.down();
  await page.mouse.move(box.x + px + 120, box.y + py, { steps: 6 });

  // Still holding the button: the engine must already know the new width.
  const during = await widthOf(page, 1);
  expect(during, "the engine sees the drag before it is released").toBeGreaterThan(before);

  // And nothing may have become undoable yet — a drag is not an edit until it
  // is let go, and one transaction per mouse-move would bury the undo stack.
  expect(await canUndo(page), "a drag in progress is not undoable").toBe(undoBefore);

  await page.mouse.up();
  await page.waitForTimeout(200);

  expect(await widthOf(page, 1), "the released width sticks").toBeGreaterThan(before);
  expect(await canUndo(page), "and one drag leaves exactly one thing to undo").toBe(true);

  expect(problems, "resizing logged nothing").toEqual([]);
});
