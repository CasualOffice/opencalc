// Dragging a column or row header to move it.
//
// Four gestures failed together in the first sweep — drag a column header, drag
// a row header, drag the selection border, and Ctrl+click for a second range —
// and it was never a UI oversight: the engine had no move primitive at all,
// only MoveSheet. `MOVE-01` built one; this is the gesture on top of it.
//
// The rule is Google Sheets': a header that is *already selected* is a move,
// anything else selects and extends. That leaves drag-to-extend exactly as it
// was, which is the gesture people use far more often, and it means the move
// only ever starts from a deliberate second grab.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    // Distinct per column and row, so a move is unambiguous.
    for (let r = 0; r < 6; r += 1) {
      for (let c = 0; c < 4; c += 1) a.session_set_cell(0, r, c, `${String.fromCharCode(65 + c)}${r + 1}`);
    }
  });
  await page.waitForTimeout(300);
}

const geo = (page) =>
  page.evaluate(() => {
    const s = window.opencalcEditor.scrollStateForTest();
    return { hw: s.bodyX0, hh: s.bodyY0 };
  });
const centre = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const ed = window.opencalcEditor;
    return { x: ed.colXAt(c) + ed.colWAt(c) / 2, y: ed.rowYAt(r) + ed.rowHAt(r) / 2 };
  }, [r, c]);
const row0 = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [0, 1, 2, 3].map((c) => a.session_cell_input(0, 0, c));
  });
const colA = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [0, 1, 2, 3].map((r) => a.session_cell_input(0, r, 0));
  });

test("dragging a selected column header moves it", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  expect(await row0(page)).toEqual(["A1", "B1", "C1", "D1"]);

  // Select column A, then grab it again and drag past C.
  await page.mouse.click(box.x + (await centre(page, 0, 0)).x, box.y + g.hh / 2);
  const from = await centre(page, 0, 0);
  const to = await centre(page, 0, 2);
  await page.mouse.move(box.x + from.x, box.y + g.hh / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x + 20, box.y + g.hh / 2, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(300);

  expect(await row0(page), "A moved past C").toEqual(["B1", "C1", "A1", "D1"]);
});

test("dragging a selected row header moves it", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);

  await page.mouse.click(box.x + g.hw / 2, box.y + (await centre(page, 0, 0)).y);
  const from = await centre(page, 0, 0);
  const to = await centre(page, 2, 0);
  await page.mouse.move(box.x + g.hw / 2, box.y + from.y);
  await page.mouse.down();
  await page.mouse.move(box.x + g.hw / 2, box.y + to.y + 10, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(300);

  expect(await colA(page), "row 1 moved past row 3").toEqual(["A2", "A3", "A1", "A4"]);
});

test("dragging a header that is not selected still extends the selection", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  const before = await row0(page);

  // No prior selection: this must select and extend, exactly as it always did.
  const from = await centre(page, 0, 0);
  const to = await centre(page, 0, 2);
  await page.mouse.move(box.x + from.x, box.y + g.hh / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x, box.y + g.hh / 2, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(200);

  const sel = await page.evaluate(() => window.opencalcEditor.selectionRectForTest());
  expect(sel.c1 - sel.c0, "three columns selected").toBe(2);
  expect(await row0(page), "and nothing moved").toEqual(before);
});

test("a move undoes in one step", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  const before = await row0(page);

  await page.mouse.click(box.x + (await centre(page, 0, 0)).x, box.y + g.hh / 2);
  const from = await centre(page, 0, 0);
  const to = await centre(page, 0, 2);
  await page.mouse.move(box.x + from.x, box.y + g.hh / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x + 20, box.y + g.hh / 2, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  expect(await row0(page)).not.toEqual(before);

  await page.locator("#grid").focus();
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  expect(await row0(page), "one undo puts it back").toEqual(before);
});

test("dropping a column on itself changes nothing and costs no undo step", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  await page.mouse.click(box.x + (await centre(page, 0, 0)).x, box.y + g.hh / 2);
  const edits = await page.evaluate(() => window.opencalcEditor.wasmApi().session_edits_applied());

  const from = await centre(page, 0, 0);
  await page.mouse.move(box.x + from.x, box.y + g.hh / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + from.x + 3, box.y + g.hh / 2, { steps: 4 });
  await page.mouse.up();
  await page.waitForTimeout(250);

  // A drag that goes nowhere must not leave a step for the user to undo past.
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_edits_applied())).toBe(edits);
});
