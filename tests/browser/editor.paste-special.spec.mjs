// Paste Special ▸ Add must skip what it cannot add to, not abandon the paste.
//
// The arithmetic modes bail out of the *whole* closure on the first cell whose
// source or target is not a number:
//
//     let CellValue::Number(src) = cc.cell.value else {
//         return (ops, cut, false);
//     };
//
// `return`, not `continue` — so everything after that cell is dropped. The
// operations gathered so far are still returned and applied, so the paste half
// happens: the cells above the label update, the ones below silently do not,
// and the status bar says `pasted add` either way.
//
// It is plainly a slip rather than a decision. The comment three lines above it
// says anything non-numeric on either side is "left alone", and the
// divide-by-zero arm in the same `match` uses `continue` — so `continue` was
// both intended and in scope.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const inputs = (page, rows, col) =>
  page.evaluate(
    ([rows, col]) => {
      const a = window.opencalcEditor.wasmApi();
      return rows.map((r) => a.session_cell_input(0, r, col));
    },
    [rows, col],
  );

test("a non-numeric cell is skipped, and the rest of the block still pastes", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    // Source column E: a number, a label, a number — the shape of any real
    // sheet where a total sits under a heading.
    a.session_set_cell(0, 0, 4, "1");
    a.session_set_cell(0, 1, 4, "label");
    a.session_set_cell(0, 2, 4, "3");
    // Target column F.
    a.session_set_cell(0, 0, 5, "10");
    a.session_set_cell(0, 1, 5, "20");
    a.session_set_cell(0, 2, 5, "30");
    a.session_clip_copy(0, 0, 4, 2, 4, false);
    a.session_clip_paste_mode(0, 0, 5, "add");
  });

  // 11 — added. 20 — skipped, because "label" is not a number. 33 — the one
  // that proves it carried on: with the early return this stayed 30, and the
  // user was told `pasted add`.
  expect(await inputs(page, [0, 1, 2], 5)).toEqual(["11", "20", "33"]);
});

test("a non-numeric target is skipped the same way", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 4, "1");
    a.session_set_cell(0, 1, 4, "2");
    a.session_set_cell(0, 2, 4, "3");
    a.session_set_cell(0, 0, 5, "10");
    a.session_set_cell(0, 1, 5, "heading"); // the target, not the source
    a.session_set_cell(0, 2, 5, "30");
    a.session_clip_copy(0, 0, 4, 2, 4, false);
    a.session_clip_paste_mode(0, 0, 5, "add");
  });
  expect(await inputs(page, [0, 1, 2], 5)).toEqual(["11", "heading", "33"]);
});

test("an empty target is still the identity, and division by zero still errors", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 4, "0");
    a.session_set_cell(0, 1, 4, "5");
    a.session_set_cell(0, 0, 5, "8");
    // F2 left empty on purpose.
    a.session_clip_copy(0, 0, 4, 1, 4, false);
    a.session_clip_paste_mode(0, 0, 5, "divide");
  });
  const [top, below] = await inputs(page, [0, 1], 5);
  // Dividing by zero gives Excel's error rather than an infinity the grid
  // cannot draw — the arm that already used `continue`, unchanged.
  expect(top).toMatch(/#DIV\/0!/);
  // And the cell after it was still reached: an empty target is the identity,
  // so 1 / 5.
  expect(below).toBe("0.2");
});
