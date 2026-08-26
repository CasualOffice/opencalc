// The Excel shortcuts a migrating user has in muscle memory.
//
// Written because I claimed six of these were broken and every one of them
// works. The probes were wrong, not the editor: one measured
// `session_active_sheet`, which reports the workbook's saved `tab_selected`
// flag — the sheet Excel opens on — rather than the sheet displayed; another
// called `selectForTest` without focusing the grid first; a third expected a
// whole-column selection to span a million rows when it spans the used range,
// which is the better behaviour.
//
// So this exists twice over: it locks in real parity that nobody had asserted,
// and it is a probe whose observables have each been checked to mean what they
// are read as saying. That second part is the reason a false alarm cost an hour.
//
// Cognitive burden is the thing being defended. A spreadsheet user's hands know
// these before they know the menus, and a shortcut that silently does nothing
// is the loudest possible way to say "this is not the tool you know".

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  // Focus first, always. Every shortcut here is bound on the canvas, and a
  // probe that skips this measures a key going to `<body>`.
  await page.locator("#grid").focus();
}

const selection = (page) =>
  page.evaluate(() => window.opencalcEditor.selectionRectForTest());
const input = (page, row, col) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [row, col]);

test("F2 opens the in-cell editor", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "hello");
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.keyboard.press("F2");
  await expect(page.locator("#inline-edit")).toBeVisible();
});

test("Alt+= sums the column above", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 3; r += 1) a.session_set_cell(0, r, 5, String(r + 1));
    window.opencalcEditor.selectForTest(3, 5);
  });
  await page.keyboard.press("Alt+Equal");
  // Excel's autosum proposes the run directly above, not the whole column.
  await expect.poll(() => input(page, 3, 5)).toBe("=SUM(F1:F3)");
});

test("Ctrl+; writes today's date", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(8, 0));
  await page.keyboard.press("Control+Semicolon");
  // The value, not a specific date: asserting today's date makes a test that
  // fails at midnight for no reason anybody can act on.
  await expect.poll(() => input(page, 8, 0)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
});

test("Ctrl+Space selects the column, Shift+Space the row", async ({ page }) => {
  await boot(page);

  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 2));
  await page.keyboard.press("Control+Space");
  const col = await selection(page);
  expect(col.c0, "one column").toBe(2);
  expect(col.c1).toBe(2);
  // The *used* range rather than a million rows — a selection nobody can see
  // the end of is not more useful than one that covers the data.
  expect(col.r1).toBeGreaterThan(col.r0);

  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 2));
  await page.keyboard.press("Shift+Space");
  const row = await selection(page);
  expect(row.r0, "one row").toBe(2);
  expect(row.r1).toBe(2);
  expect(row.c1).toBeGreaterThan(row.c0);
});

test("Ctrl+` shows formulas instead of results", async ({ page }) => {
  await boot(page);
  const showing = () => page.evaluate(() => !!window.opencalcEditor.viewOptions().formulas);
  expect(await showing()).toBe(false);
  await page.keyboard.press("Control+Backquote");
  await expect.poll(showing).toBe(true);
  // It is a toggle, so pressing it again has to put the sheet back.
  await page.keyboard.press("Control+Backquote");
  await expect.poll(showing).toBe(false);
});

test("Ctrl+PageDown and Ctrl+PageUp move between sheets", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_add_sheet("S2"));
  await page.evaluate(() => window.opencalcEditor.switchSheet(0));
  await page.locator("#grid").focus();

  // The active *tab*, which is what a user sees. `session_active_sheet` reports
  // the workbook's saved `tab_selected` — the sheet a reader opens on — and
  // reading it here is what produced a false failure.
  const tab = () => page.locator(".sheet-tab.active").innerText();

  await page.keyboard.press("Control+PageDown");
  await expect.poll(tab).toBe("Sheet2");
  await page.keyboard.press("Control+PageUp");
  await expect.poll(tab).toBe("Sheet1");
});
