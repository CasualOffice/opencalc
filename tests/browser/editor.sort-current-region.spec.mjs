// Sort acts on the current region, and asks before leaving data behind
// (XL-UX-175).
//
// Excel's A→Z from a single cell sorts the *current region* around it — the
// block bounded by blank rows and columns, the one Ctrl+A's first press and
// Ctrl+Shift+* select — and guesses whether its first row is a header. The
// editor sorted the whole used area from A1 instead, so a second table across a
// blank column was reordered by a key column it does not have.
//
// A selection that stops short of its region (one column of a table) gets
// Excel's Sort Warning: "…found data next to your selection…", with *Expand
// the selection* and *Continue with the current selection*, Sort and Cancel.
// Without it, sorting one column silently tore every row of the table apart.

import { expect, test } from "@playwright/test";

/// Block 1 is A1:B5 with a header; block 2 is D1:D4, across the blank column C.
async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    [["Name", "Score"], ["dan", "4"], ["amy", "1"], ["cat", "3"], ["bob", "2"]]
      .forEach((row, r) => row.forEach((v, c) => a.session_set_cell(0, r, c, v)));
    ["9", "7", "8", "6"].forEach((v, r) => a.session_set_cell(0, r, 3, v));
    ed.selectForTest(0, 0);
    ed.draw();
  });
  await page.locator("#grid").focus();
}

const column = (page, col, rows) =>
  page.evaluate(([col, rows]) => {
    const a = window.opencalcEditor.wasmApi();
    return Array.from({ length: rows }, (_, r) => a.session_cell_input(0, r, col));
  }, [col, rows]);
const run = (page, id) => page.evaluate((id) => { window.opencalcEditor.runCommand(id); }, id);
const box = (page) => page.locator("#oc-msgbox");

test("sorting from one cell sorts its region and leaves the block across a blank column alone", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 1)); // B3
  await run(page, "data.sort-range.a-z");
  await page.waitForTimeout(150);

  expect(await column(page, 1, 5), "block 1 sorted by Score, header kept").toEqual(["Score", "1", "2", "3", "4"]);
  expect(await column(page, 0, 5), "the names travelled with their scores").toEqual(["Name", "amy", "bob", "cat", "dan"]);
  expect(await column(page, 3, 4), "block 2 is a different region and was not touched").toEqual(["9", "7", "8", "6"]);
  await expect(box(page), "a single cell is its whole region: nothing to warn about").toBeHidden();
});

test("one column of a table raises the Sort Warning, and Expand sorts the whole region", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(0, 1);
    ed.extendSelectionForTest(4, 1); // B1:B5 — the Score column only
  });
  await run(page, "data.sort-range.a-z");

  await expect(box(page)).toBeVisible();
  await expect(page.locator("#oc-msgbox-title")).toHaveText("Sort Warning");
  await expect(page.locator("#oc-msgbox-text")).toContainText("found data next to your selection");
  await expect(page.locator("#oc-msgbox-text")).toContainText("it will not be sorted");
  const expand = box(page).getByLabel("Expand the selection");
  const keep = box(page).getByLabel("Continue with the current selection");
  await expect(expand, "Expand is Excel's default").toBeChecked();
  await expect(keep).not.toBeChecked();
  await expect(page.locator("#oc-msgbox-sort")).toBeFocused();
  await expect(page.locator("#oc-msgbox-cancel")).toBeVisible();
  expect(await column(page, 1, 5), "nothing moves while the question is open").toEqual(["Score", "4", "1", "3", "2"]);

  await page.keyboard.press("Enter");
  await expect(box(page)).toBeHidden();
  expect(await column(page, 1, 5)).toEqual(["Score", "1", "2", "3", "4"]);
  expect(await column(page, 0, 5), "expanded: the names moved with the scores").toEqual(["Name", "amy", "bob", "cat", "dan"]);
  expect(await column(page, 3, 4), "expansion stops at the blank column").toEqual(["9", "7", "8", "6"]);
});

test("Continue with the current selection sorts only what was selected", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(0, 1);
    ed.extendSelectionForTest(4, 1);
  });
  await run(page, "data.sort-range.a-z");
  await expect(box(page)).toBeVisible();
  // The arrow keys move between the two answers, as in Excel's dialog.
  await box(page).getByLabel("Expand the selection").focus();
  await page.keyboard.press("ArrowDown");
  await expect(box(page).getByLabel("Continue with the current selection")).toBeChecked();
  await page.locator("#oc-msgbox-sort").click();
  await expect(box(page)).toBeHidden();

  expect(await column(page, 1, 5)).toEqual(["Score", "1", "2", "3", "4"]);
  expect(await column(page, 0, 5), "the names stayed where they were").toEqual(["Name", "dan", "amy", "cat", "bob"]);
});

test("Cancel on the Sort Warning sorts nothing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(0, 1);
    ed.extendSelectionForTest(4, 1);
  });
  await run(page, "data.sort-range.z-a");
  await expect(box(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(box(page)).toBeHidden();
  expect(await column(page, 1, 5)).toEqual(["Score", "4", "1", "3", "2"]);
  expect(await column(page, 0, 5)).toEqual(["Name", "dan", "amy", "cat", "bob"]);
});

test("the Sort dialog asks the same question first, then opens on the expanded region", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(1, 1);
    ed.extendSelectionForTest(4, 1);
  });
  await run(page, "data.sort-range.custom-sort");
  await expect(box(page)).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.locator("#oc-modal")).toBeVisible();
  await expect(page.locator("#oc-modal-body")).toContainText("A1:B5");
});

test("a whole table selected exactly is sorted without a warning", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(0, 1);
    ed.extendSelectionForTest(4, 0);
  });
  await run(page, "data.sort-range.a-z");
  await page.waitForTimeout(150);
  await expect(box(page)).toBeHidden();
  expect(await column(page, 1, 5)).toEqual(["Score", "1", "2", "3", "4"]);
  expect(await column(page, 3, 4)).toEqual(["9", "7", "8", "6"]);
});
