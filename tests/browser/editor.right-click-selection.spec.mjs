// A right-click does not throw away the selection it was aimed at.
//
// Reported from a running editor: Ctrl+A took the block, a right-click inside
// it left one cell selected, and every verb in the menu then acted on that cell
// rather than on what the user was looking at — Delete included.
//
// The cause was ordering, not policy. `mousedown` fires *before* `contextmenu`,
// so the right button reached the ordinary select path and collapsed the
// selection before the menu opened. The `contextmenu` handler already had the
// right rule — keep a selection you clicked inside, move one you clicked
// outside, which is what Excel and Sheets both do — and never got the chance to
// apply it.
//
// Both halves are asserted here. Fixing the first by making the right button
// inert everywhere would have broken the second, and the second is the commoner
// gesture: right-clicking a cell you have not selected yet.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    const hdr = ["Region", "Rep", "Units", "Revenue"];
    hdr.forEach((h, c) => a.session_set_cell(0, 0, c, h));
    for (let r = 1; r < 7; r += 1) {
      hdr.forEach((_, c) => a.session_set_cell(0, r, c, c < 2 ? `v${r}${c}` : String(r * 10 + c)));
    }
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.waitForTimeout(250);
}

const centre = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const e = window.opencalcEditor;
    return { x: e.colXAt(c) + e.colWAt(c) / 2, y: e.rowYAt(r) + e.rowHAt(r) / 2 };
  }, [r, c]);
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());

test("right-clicking inside a select-all keeps the whole selection", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+a");
  await page.waitForTimeout(200);
  const all = await sel(page);
  expect(all.r1 - all.r0, "Ctrl+A took more than one row").toBeGreaterThan(0);

  const p = await centre(page, 3, 2);
  await page.mouse.click(box.x + p.x, box.y + p.y, { button: "right" });
  await page.waitForTimeout(250);

  expect(await sel(page), "the selection survived the right-click").toEqual(all);
  await expect(page.locator("#sheet-ctx")).toBeVisible();
});

test("right-clicking inside a dragged range keeps it", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const a = await centre(page, 1, 0), b = await centre(page, 4, 2);
  await page.mouse.move(box.x + a.x, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + b.x, box.y + b.y, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(200);
  const range = await sel(page);
  expect(range.c1 - range.c0, "a range was dragged").toBeGreaterThan(0);

  const inside = await centre(page, 2, 1);
  await page.mouse.click(box.x + inside.x, box.y + inside.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await sel(page), "the dragged range survived").toEqual(range);
});

test("right-clicking outside a selection still moves it", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const near = await centre(page, 1, 1);
  await page.mouse.click(box.x + near.x, box.y + near.y);
  await page.waitForTimeout(150);

  const far = await centre(page, 5, 3);
  await page.mouse.click(box.x + far.x, box.y + far.y, { button: "right" });
  await page.waitForTimeout(250);
  // The commoner gesture, and the one a blanket "ignore the right button" fix
  // would have broken: right-clicking a cell you have not selected yet.
  expect(await sel(page)).toEqual({ r0: 5, c0: 3, r1: 5, c1: 3 });
});

// The four selection kinds reach four different branches of the right-click
// handler, and only two of them were exercised by the report. `SEC-022` is what
// this class costs when it is not caught: *"Remove Duplicates destroyed data
// the user never selected"*. The handler's own header branches already carry a
// comment about it — "every verb in it then acted on the wrong target,
// including Delete" — so the rule was known and only partly enforced.
test("a right-click preserves every kind of selection it lands inside", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await page.evaluate(() => {
    const s = window.opencalcEditor.scrollStateForTest();
    return { hw: s.bodyX0, hh: s.bodyY0 };
  });

  // A column band: click its header, then right-click a cell inside it.
  const colX = (await centre(page, 0, 1)).x;
  await page.mouse.click(box.x + colX, box.y + g.hh / 2);
  await page.waitForTimeout(150);
  const cols = await sel(page);
  const inCol = await centre(page, 3, 1);
  await page.mouse.click(box.x + inCol.x, box.y + inCol.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await sel(page), "a column selection survived").toEqual(cols);
  await page.keyboard.press("Escape");

  // A row band, the same way.
  const rowY = (await centre(page, 2, 0)).y;
  await page.mouse.click(box.x + g.hw / 2, box.y + rowY);
  await page.waitForTimeout(150);
  const rows = await sel(page);
  const inRow = await centre(page, 2, 2);
  await page.mouse.click(box.x + inRow.x, box.y + inRow.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await sel(page), "a row selection survived").toEqual(rows);
});

// PIPE-08. A multi-range selection is several rectangles, and the right-click
// rule asked only about the *active* one (`selRect`). Right-clicking inside a
// range banked by an earlier Ctrl+click therefore read as "outside", collapsed
// the selection to one cell, and the menu's verbs acted on that cell.
const cellsOf = (page) => page.evaluate(() => {
  const out = new Set();
  for (const g of window.opencalcEditor.allRanges()) {
    for (let r = g.r0; r <= g.r1; r += 1) for (let c = g.c0; c <= g.c1; c += 1) out.add(`${r},${c}`);
  }
  return [...out].sort();
});

async function dragRange(page, box, from, to) {
  const a = await centre(page, ...from), b = await centre(page, ...to);
  await page.mouse.move(box.x + a.x, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + b.x, box.y + b.y, { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(150);
}

async function ctrlClick(page, box, r, c) {
  const p = await centre(page, r, c);
  await page.keyboard.down("Control");
  await page.mouse.click(box.x + p.x, box.y + p.y);
  await page.keyboard.up("Control");
  await page.waitForTimeout(150);
}

test("right-clicking inside an earlier Ctrl+click range keeps the whole multi-range", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await dragRange(page, box, [1, 0], [2, 1]); // A2:B3
  await ctrlClick(page, box, 5, 3);           // + D6, now the active range
  const before = await cellsOf(page);
  expect(before, "two ranges are selected").toEqual(["1,0", "1,1", "2,0", "2,1", "5,3"]);

  const inBanked = await centre(page, 2, 1);  // B3, in the banked range
  await page.mouse.click(box.x + inBanked.x, box.y + inBanked.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await cellsOf(page), "the multi-range survived the right-click").toEqual(before);
  await expect(page.locator("#sheet-ctx")).toBeVisible();
});

// Excel 365: "While pressing the Ctrl key, you can click … to deselect any
// cells … within a selection" (support.microsoft.com, "Deselect a selection").
// The editor dropped a whole banked range for a Ctrl+click on one of its cells,
// and inside the active range it banked a duplicate instead of deselecting.
test("Ctrl+click on a cell of the active range deselects that cell only", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await dragRange(page, box, [1, 0], [2, 1]); // A2:B3
  await ctrlClick(page, box, 2, 1);           // B3
  expect(await cellsOf(page)).toEqual(["1,0", "1,1", "2,0"]);
});

test("Ctrl+click on a cell of a banked range deselects that cell, not the range", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await dragRange(page, box, [1, 0], [2, 1]); // A2:B3
  await ctrlClick(page, box, 5, 3);           // + D6
  await ctrlClick(page, box, 1, 0);           // A2, in the banked range
  expect(await cellsOf(page)).toEqual(["1,1", "2,0", "2,1", "5,3"]);
  // The active cell is still a selected one: typing must not land in a cell
  // the selection no longer shows.
  const active = await page.evaluate(() => window.opencalcEditor.scrollStateForTest());
  expect(await cellsOf(page)).toContain(`${active.row},${active.col}`);
});

// Found while fixing the above. Ctrl+A with the active cell inside the block
// *holds* the block (the active cell is not one of its corners), and that hold
// went on answering for the active range after a Ctrl+click: the new cell was
// never selected and the block was banked twice.
test("Ctrl+click after Ctrl+A adds the clicked cell as a new range", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  await page.evaluate(() => window.opencalcEditor.selectForTest(3, 2));
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+a");
  await page.waitForTimeout(150);
  const block = await sel(page);
  await ctrlClick(page, box, 9, 6); // G10, outside the block
  const ranges = await page.evaluate(() => window.opencalcEditor.allRanges());
  expect(ranges).toEqual([block, { r0: 9, c0: 6, r1: 9, c1: 6 }]);
});

// PIPE-08, the header half. Ctrl+clicking column headers banks whole-column
// bands, and a right-click on the header of a banked band collapsed the
// selection to that one column: the right button's `mousedown` reached the
// ordinary header select before the menu opened, and the menu's own check
// asked only about the active band. "Delete 2 columns" then could not be
// asked for at all. [unverified] from memory: Excel keeps a non-adjacent
// column selection on a header right-click, and Delete removes every selected
// column as one step.
const headerAt = async (page, axis, i) => {
  const g = await page.evaluate(() => {
    const s = window.opencalcEditor.scrollStateForTest();
    return { hw: s.bodyX0, hh: s.bodyY0 };
  });
  const p = await centre(page, axis === "col" ? 0 : i, axis === "col" ? i : 0);
  return axis === "col" ? { x: p.x, y: g.hh / 2 } : { x: g.hw / 2, y: p.y };
};

async function ctrlClickHeader(page, box, axis, i) {
  const p = await headerAt(page, axis, i);
  await page.keyboard.down("Control");
  await page.mouse.click(box.x + p.x, box.y + p.y);
  await page.keyboard.up("Control");
  await page.waitForTimeout(150);
}

const row0 = (page) => page.evaluate(() => {
  const a = window.opencalcEditor.wasmApi();
  return [0, 1, 2, 3].map((c) => JSON.parse(a.session_cells(0, 0, c, 0, c))[0]?.t ?? "");
});

test("right-clicking a banked column header keeps every column, and Delete removes them all", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const b = await headerAt(page, "col", 1);
  await page.mouse.click(box.x + b.x, box.y + b.y); // column B
  await page.waitForTimeout(150);
  await ctrlClickHeader(page, box, "col", 3);         // + column D, now active
  const before = await page.evaluate(() => window.opencalcEditor.allRanges().map((g) => [g.c0, g.c1]));
  expect(before, "two column bands are selected").toEqual([[1, 1], [3, 3]]);

  await page.mouse.click(box.x + b.x, box.y + b.y, { button: "right" }); // banked B
  await page.waitForTimeout(250);
  expect(await page.evaluate(() => window.opencalcEditor.allRanges().map((g) => [g.c0, g.c1])),
    "both bands survived the right-click").toEqual(before);
  // Excel's word (XL-UX-034): "Delete", whatever the count — every band goes.
  const del = page.locator("#sheet-ctx button", { hasText: /^Delete$/ });
  await expect(del).toBeVisible();
  await del.click();
  await page.waitForTimeout(200);
  expect(await row0(page), "B and D are gone, A and C remain").toEqual(["Region", "Units", "", ""]);

  // One step: a single undo brings both back.
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  expect(await row0(page)).toEqual(["Region", "Rep", "Units", "Revenue"]);
});

test("right-clicking a banked row header keeps every row", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const r = await headerAt(page, "row", 2);
  await page.mouse.click(box.x + r.x, box.y + r.y); // row 3
  await page.waitForTimeout(150);
  await ctrlClickHeader(page, box, "row", 5);         // + row 6
  const before = await page.evaluate(() => window.opencalcEditor.allRanges().map((g) => [g.r0, g.r1]));
  expect(before).toEqual([[2, 2], [5, 5]]);
  await page.mouse.click(box.x + r.x, box.y + r.y, { button: "right" });
  await page.waitForTimeout(250);
  expect(await page.evaluate(() => window.opencalcEditor.allRanges().map((g) => [g.r0, g.r1]))).toEqual(before);
  await expect(page.locator("#sheet-ctx button", { hasText: /^Delete$/ })).toBeVisible();
});
