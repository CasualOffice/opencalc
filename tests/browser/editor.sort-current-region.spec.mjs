// Sort works on Excel's current region (XL-UX-175).
//
// From one selected cell, Excel's Sort A to Z / Z to A / Sort… act on the
// current region around the cell — the block bounded by blank rows and blank
// columns, what Ctrl+Shift+* selects. OpenCalc sorted the whole used area from
// A1 instead, so a second block across a blank column was reordered by a key it
// does not have, and a title above a blank row was sorted into the data.
//
// A selection of more than one cell with data in a column beside it raises
// Excel's Sort Warning — "Expand the selection" or "Continue with the current
// selection", Sort or Cancel — where OpenCalc sorted without asking.
// (support.microsoft.com: "Sort data in a range or table"; "Expanding a
// selection while sorting data or removing duplicates".)

import { expect, test } from "@playwright/test";

async function boot(page, cells) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate((cells) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (const [r, c, v] of cells) a.session_set_cell(0, r, c, v);
    window.opencalcEditor.selectForTest(0, 0);
  }, cells);
  await page.locator("#grid").focus();
}

/// The text of rows r0..r1 over columns c0..c1, row by row.
const grid = (page, r0, c0, r1, c1) => page.evaluate(([r0, c0, r1, c1]) => {
  const a = window.opencalcEditor.wasmApi();
  const out = [];
  for (let r = r0; r <= r1; r += 1) {
    const row = [];
    for (let c = c0; c <= c1; c += 1) row.push(a.session_cell_input(0, r, c));
    out.push(row);
  }
  return out;
}, [r0, c0, r1, c1]);

const select = (page, r0, c0, r1 = r0, c1 = c0) => page.evaluate(([r0, c0, r1, c1]) => {
  window.opencalcEditor.selectForTest(r0, c0);
  if (r1 !== r0 || c1 !== c0) window.opencalcEditor.extendSelectionForTest(r1, c1);
}, [r0, c0, r1, c1]);

const run = (page, id) => page.evaluate((id) => window.opencalcEditor.runCommand(id), id);

///        A      B       C   D      E
///   1    Name   Score       City   Pop
///   2    c      3           z      30
///   3    a      1           x      10
///   4    b      2           y      20
const TWO_BLOCKS = [
  [0, 0, "Name"], [0, 1, "Score"], [0, 3, "City"], [0, 4, "Pop"],
  [1, 0, "c"], [1, 1, "3"], [1, 3, "z"], [1, 4, "30"],
  [2, 0, "a"], [2, 1, "1"], [2, 3, "x"], [2, 4, "10"],
  [3, 0, "b"], [3, 1, "2"], [3, 3, "y"], [3, 4, "20"],
];
const BLOCK2 = [["City", "Pop"], ["z", "30"], ["x", "10"], ["y", "20"]];

test("from one cell, sorting block 1 leaves block 2 across a blank column untouched", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1);
  await run(page, "data.sort-range.a-z");
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["a", "1"], ["b", "2"], ["c", "3"]]);
  expect(await grid(page, 0, 3, 3, 4)).toEqual(BLOCK2);
});

test("from one cell, Z→A sorts the current region the same way", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 2, 4);
  await run(page, "data.sort-range.z-a");
  expect(await grid(page, 0, 3, 3, 4)).toEqual([["City", "Pop"], ["z", "30"], ["y", "20"], ["x", "10"]]);
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["c", "3"], ["a", "1"], ["b", "2"]]);
});

///        A       B       C
///   1    Report
///   2
///   3            Item    Qty
///   4            pear    3
///   5            apple   1
///   6            fig     2
test("a single cell sorts its region, not from A1, and keeps the region's header row", async ({ page }) => {
  await boot(page, [
    [0, 0, "Report"],
    [2, 1, "Item"], [2, 2, "Qty"],
    [3, 1, "pear"], [3, 2, "3"],
    [4, 1, "apple"], [4, 2, "1"],
    [5, 1, "fig"], [5, 2, "2"],
  ]);
  await select(page, 4, 2);
  await run(page, "data.sort-range.a-z");
  expect(await grid(page, 0, 0, 5, 2)).toEqual([
    ["Report", "", ""],
    ["", "", ""],
    ["", "Item", "Qty"],
    ["", "apple", "1"],
    ["", "fig", "2"],
    ["", "pear", "3"],
  ]);
});

test("one sort from one cell is one undo step", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1);
  await run(page, "data.sort-range.a-z");
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+z");
  expect(await grid(page, 0, 0, 3, 4)).toEqual(
    TWO_BLOCKS.reduce((g, [r, c, v]) => { g[r][c] = v; return g; },
      Array.from({ length: 4 }, () => ["", "", "", "", ""])),
  );
});

test("Custom Sort from one cell offers the current region", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 4);
  await run(page, "data.sort-range.custom-sort");
  await expect(page.locator("#oc-modal-title")).toHaveText("Sort range");
  await expect(page.locator("#oc-modal .oc-confirm-text").first()).toHaveText(/^D1:E4\b/);
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await grid(page, 0, 3, 3, 4)).toEqual([["City", "Pop"], ["x", "10"], ["y", "20"], ["z", "30"]]);
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["c", "3"], ["a", "1"], ["b", "2"]]);
});

// ---- Sort Warning -----------------------------------------------------------

const warning = (page) => page.locator("#oc-modal");
async function expectWarning(page) {
  await expect(page.locator("#oc-modal-title")).toHaveText("Sort Warning");
  await expect(warning(page)).toContainText("found data next to your selection");
  await expect(warning(page).locator(".oc-radio", { hasText: "Expand the selection" }).locator("input")).toBeChecked();
  await expect(warning(page).locator(".oc-radio", { hasText: "Continue with the current selection" })).toBeVisible();
}

test("a partial selection with data beside it raises the Sort Warning; Expand sorts the region", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1, 3, 1); // B2:B4, the scores only
  await run(page, "data.sort-range.a-z");
  await expectWarning(page);
  await warning(page).locator(".oc-btn.primary", { hasText: "Sort" }).click();
  await expect(warning(page)).toBeHidden();
  // The names travelled with their scores, the header stayed, block 2 did not move.
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["a", "1"], ["b", "2"], ["c", "3"]]);
  expect(await grid(page, 0, 3, 3, 4)).toEqual(BLOCK2);
});

test("Continue with the current selection sorts only the selected cells", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1, 3, 1);
  await run(page, "data.sort-range.a-z");
  await expectWarning(page);
  await warning(page).locator(".oc-radio", { hasText: "Continue with the current selection" }).locator("input").check();
  await warning(page).locator(".oc-btn.primary", { hasText: "Sort" }).click();
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["c", "1"], ["a", "2"], ["b", "3"]]);
  expect(await grid(page, 0, 3, 3, 4)).toEqual(BLOCK2);
});

test("Cancel on the Sort Warning sorts nothing", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1, 3, 1);
  await run(page, "data.sort-range.z-a");
  await expectWarning(page);
  await warning(page).locator(".oc-btn", { hasText: "Cancel" }).click();
  await expect(warning(page)).toBeHidden();
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["c", "3"], ["a", "1"], ["b", "2"]]);
});

test("Escape on the Sort Warning sorts nothing", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1, 3, 1);
  await run(page, "data.sort-range.a-z");
  await expectWarning(page);
  await page.keyboard.press("Escape");
  await expect(warning(page)).toBeHidden();
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["c", "3"], ["a", "1"], ["b", "2"]]);
});

test("Custom Sort on a partial selection warns first, then opens on the expanded region", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 1, 3, 1);
  await run(page, "data.sort-range.custom-sort");
  await expectWarning(page);
  await warning(page).locator(".oc-btn.primary", { hasText: "Sort" }).click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Sort range");
  await expect(page.locator("#oc-modal .oc-confirm-text").first()).toHaveText(/^A1:B4\b/);
});

test("a selection with no data beside it sorts without a warning", async ({ page }) => {
  await boot(page, TWO_BLOCKS);
  await select(page, 1, 0, 3, 1); // A2:B4, the whole body of block 1
  await run(page, "data.sort-range.a-z");
  await expect(page.locator("#oc-modal")).toBeHidden();
  expect(await grid(page, 0, 0, 3, 1)).toEqual([["Name", "Score"], ["a", "1"], ["b", "2"], ["c", "3"]]);
});

test("inside a table, one cell sorts the table body and leaves the totals row", async ({ page }) => {
  await boot(page, [
    [0, 0, "Item"], [0, 1, "Qty"],
    [1, 0, "pear"], [1, 1, "3"],
    [2, 0, "apple"], [2, 1, "1"],
    [3, 0, "fig"], [3, 1, "2"],
    [0, 3, "keep"], [1, 3, "me"],
  ]);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_create_table(0, 0, 0, 3, 1, "Fruit", true);
    a.session_table_totals(0, 0, 0, true);
    a.session_set_cell(0, 4, 0, "Total");
  });
  await select(page, 2, 0);
  // Z→A, so a totals row swept into the sort would land on top, not by
  // coincidence at the bottom where "Total" sorts anyway.
  await run(page, "data.sort-range.z-a");
  expect(await grid(page, 0, 0, 4, 0)).toEqual([["Item"], ["pear"], ["fig"], ["apple"], ["Total"]]);
  expect(await grid(page, 0, 3, 1, 3)).toEqual([["keep"], ["me"]]);
});
