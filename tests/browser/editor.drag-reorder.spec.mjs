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

// --- what the drag looks like while it is happening -------------------------
//
// The gesture worked and told the user almost nothing: a blue line at the drop
// boundary was the whole of it. The cursor stayed `cell` for the entire drag,
// and the band being dragged was drawn exactly as it had been — so the line
// said *where* and nothing said *what*. Excel and Sheets both lift the band
// into a translucent copy that follows the pointer and leave the source looking
// vacated; these three assert that this one does too.
//
// The pixel check reads the **column header strip**, not the cells. A selected
// header is tinted, so lifting it puts a band of tinted pixels over a run of
// plain header background — a flat, unambiguous block roughly a column wide,
// where the cell area is mostly background copied onto background and only the
// glyphs differ. The drop line, which existed before any of this, is three
// pixels wide: it cannot account for a changed run of dozens.

/// The colours along a horizontal run of the canvas, in canvas-logical px.
const scanRow = (page, x0, x1, y) =>
  page.evaluate(([x0, x1, y]) => {
    const c = document.getElementById("grid");
    // The bitmap is `dpr` times the CSS box; getImageData works in bitmap px.
    const k = c.width / parseFloat(getComputedStyle(c).width);
    const cx = c.getContext("2d");
    const out = [];
    for (let x = Math.round(x0); x <= Math.round(x1); x += 1) {
      const d = cx.getImageData(Math.round(x * k), Math.round(y * k), 1, 1).data;
      out.push([d[0], d[1], d[2]]);
    }
    return out;
  }, [x0, x1, y]);

const changedPx = (a, b) =>
  a.filter((p, i) => p.some((ch, j) => Math.abs(ch - b[i][j]) > 6)).length;

const cursorOf = (page) =>
  page.evaluate(() => getComputedStyle(document.getElementById("grid")).cursor);

test("the dragged band is lifted, and the cursor says a drag is happening", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  const from = await centre(page, 0, 0);
  const to = await centre(page, 0, 5);
  const w = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  // The run the ghost will cover: it keeps the grip it was taken by, so a band
  // grabbed at its centre lands centred on the pointer.
  const runStart = to.x - w / 2 + 3;
  const runEnd = to.x + w / 2 - 3;
  const hdrY = g.hh / 2;

  await page.mouse.click(box.x + from.x, box.y + hdrY);
  await page.waitForTimeout(120);
  const before = await scanRow(page, runStart, runEnd, hdrY);

  await page.mouse.move(box.x + from.x, box.y + hdrY);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x, box.y + hdrY, { steps: 12 });
  await page.waitForTimeout(120);

  expect(await cursorOf(page), "the pointer says a drag is in progress").toBe("grabbing");

  // The pixels first: they are the evidence that something was *drawn*, and
  // they are what a user sees. The hook below only corroborates it.
  const during = await scanRow(page, runStart, runEnd, hdrY);
  expect(changedPx(during, before),
    `header pixels repainted under the pointer (of ${before.length})`).toBeGreaterThan(20);

  // Reported at the end of the paint that drew it, so this cannot be true of a
  // drag whose ghost was never painted.
  const ghost = await page.evaluate(() => window.opencalcEditor.moveGhostForTest());
  expect(ghost, "a ghost was painted this frame").not.toBeNull();
  expect(ghost.axis).toBe("col");
  expect(Math.abs(ghost.x + ghost.w / 2 - to.x), "and it tracks the pointer").toBeLessThan(4);

  await page.mouse.up();
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => window.opencalcEditor.moveGhostForTest()),
    "and nothing is left floating after the drop").toBeNull();
});

test("the source band is marked while it is being dragged", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  const from = await centre(page, 0, 0);
  const to = await centre(page, 0, 5);
  const cell0 = await centre(page, 1, 0);
  const w = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  const runStart = from.x - w / 2 + 4;
  const runEnd = from.x + w / 2 - 4;

  await page.mouse.click(box.x + from.x, box.y + g.hh / 2);
  await page.waitForTimeout(120);
  // A run across column A's own cells, well away from both the pointer and the
  // drop line: nothing but the source marking can change these.
  const before = await scanRow(page, runStart, runEnd, cell0.y);

  await page.mouse.move(box.x + from.x, box.y + g.hh / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + to.x, box.y + g.hh / 2, { steps: 12 });
  await page.waitForTimeout(120);
  const during = await scanRow(page, runStart, runEnd, cell0.y);

  expect(changedPx(during, before), "the band being moved no longer looks untouched")
    .toBeGreaterThan(20);
  await page.mouse.up();
});

test("hovering a header that would move shows the grab cursor", async ({ page }) => {
  await boot(page);
  const box = await page.locator("#grid").boundingBox();
  const g = await geo(page);
  const a = await centre(page, 0, 0);
  const c = await centre(page, 0, 2);

  // Nothing selected: this header starts a selection, not a move.
  await page.mouse.move(box.x + a.x, box.y + g.hh / 2);
  await page.waitForTimeout(80);
  expect(await cursorOf(page)).toBe("cell");

  await page.mouse.click(box.x + a.x, box.y + g.hh / 2);
  // Off and back on, so the cursor is recomputed by a real hover.
  await page.mouse.move(box.x + c.x, box.y + g.hh / 2);
  await page.mouse.move(box.x + a.x, box.y + g.hh / 2);
  await page.waitForTimeout(80);
  expect(await cursorOf(page), "the selected band offers itself to be dragged").toBe("grab");
});
