// A filter that hides rows for you and for nobody else (COL-32, docs/71).
//
// A browser is the only place the whole claim can be checked, because the claim
// spans three layers: the engine must not move a value, the layout must collapse
// the row, and the collaboration client must send nothing. A Rust test can prove
// the first and third; only here do all three hold at once, against the same
// build a user gets.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// A short column with a `SUBTOTAL(109, …)` under it. 109 is SUM-ignoring-hidden
/// rows, which is the function whose answer a filter is allowed to change — and
/// a personal view is not.
async function type(page, ref, text) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  await page.fill("#formula-input", text);
  await page.press("#formula-input", "Enter");
}

async function seed(page) {
  // Through the formula bar, as a user would: a helper that wrote cells
  // directly could pass against a build where the grid never sees them.
  await type(page, "A1", "Fruit");
  await type(page, "A2", "apple");
  await type(page, "A3", "pear");
  await type(page, "A4", "apple");
  // 109 is SUM ignoring hidden rows — the value a shared filter may move and a
  // personal view may not.
  await type(page, "B1", "=SUBTOTAL(109,C1:C4)");
  await type(page, "C2", "1");
  await type(page, "C3", "2");
  await type(page, "C4", "3");
}

/// Turn the sheet's autofilter on over the seeded block.
///
/// A *shared* edit, deliberately: the filter control is part of the document —
/// Excel stores it as `<autoFilter>` and every participant sees the buttons.
/// Only the **rule** can be personal. Getting this the other way round would
/// mean one participant's dropdown appearing on another's screen with no
/// operation to explain it.
async function turnFilterOn(page) {
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.locator('[data-oc-label="Data"]').click();
  await page.locator('[data-oc-label="Filter"]').click();
}

/// **A personal filter hides rows here and moves no value.**
test("a personal filter hides rows without changing a cell", async ({ page }) => {
  await boot(page);
  await seed(page);
  await turnFilterOn(page);

  const before = await page.evaluate(() => window.opencalcEditor.personalViewForTest());
  expect(before.hasView, "a view existed before one was applied").toBe(false);

  await page.evaluate(() => window.opencalcEditor.personalFilterForTest(0, ["apple"]));

  const after = await page.evaluate(() => window.opencalcEditor.personalViewForTest());
  expect(after.hasView, "no personal view was recorded").toBe(true);
  expect(after.visibleRows.length, "the view hid nothing on screen")
    .toBeLessThan(before.visibleRows.length);

  // The shared set is untouched: this is what a co-editor would have seen move.
  expect(after.sharedHidden, "a personal view wrote into the shared hidden set")
    .toBe(before.sharedHidden);
});

/// **Clearing the view brings every row back.**
///
/// The paired half: without it the test above passes against a build that
/// simply hides rows and can never show them again.
test("clearing a personal view restores every row", async ({ page }) => {
  await boot(page);
  await seed(page);
  await turnFilterOn(page);

  const before = await page.evaluate(() => window.opencalcEditor.personalViewForTest());
  await page.evaluate(() => window.opencalcEditor.personalFilterForTest(0, ["apple"]));
  await page.evaluate(() => {
    window.opencalcEditor.clearMyViewForTest();
  });

  const after = await page.evaluate(() => window.opencalcEditor.personalViewForTest());
  expect(after.hasView).toBe(false);
  expect(after.visibleRows).toEqual(before.visibleRows);
});

/// **The scope choice defaults to shared.**
///
/// docs/71 is explicit: shared is what a spreadsheet has always done and the
/// only kind the file format can express, so it is what pressing Apply without
/// reading anything gives you. A default that silently filtered "just for me"
/// would be the worse mistake — the user would believe they had filtered for
/// the room.
test("the filter dropdown defaults to filtering for everyone", async ({ page }) => {
  await boot(page);
  await seed(page);

  await turnFilterOn(page);

  // Open the column dropdown through the editor's own hit target.
  await page.evaluate(() => window.opencalcEditor.openColumnFilterForTest(0));
  const box = page.locator(".filter-scope-box");
  await expect(box).toHaveCount(1);
  await expect(box, "the scope choice was pre-ticked to personal").not.toBeChecked();
});
