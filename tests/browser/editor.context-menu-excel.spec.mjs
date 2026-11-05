// The cell, row-header and column-header context menus are Excel's
// (fit-gap XL-UX-034).
//
// Excel for Microsoft 365 on Windows, right-click on a cell: Cut, Copy, Paste
// Options: (a row of icons), Paste Special…, Insert…, Delete…, Clear Contents,
// Filter ›, Sort ›, New Comment, Format Cells…, Pick From Drop-down List…,
// Define Name…, Link ›. On a row or column header: Cut, Copy, Paste Options:,
// Paste Special…, Insert, Delete, Clear Contents, Format Cells…, Row Height… /
// Column Width…, Hide, Unhide. [unverified] from memory of Excel 365, version
// 2410; Microsoft documents the items one by one, not the menu as a list.
//
// Ours was sentence case and its own shape: "Insert cells ›" *and* "Insert ›",
// "Delete cells ›" *and* "Delete ›", a "Hide ›" and a "Clear ›" Excel's cell
// menu does not have, "Insert comment" and "Create table…". Somebody who knows
// Excel reaches for Insert… and finds two Inserts, neither of which is the
// dialog Ctrl++ opens.
//
// Left out on purpose, because OpenCalc does not have the feature (docs/95
// rule 9 does not let a menu promise what the app cannot do): Smart Lookup,
// Quick Analysis, Get Data from Table/Range, New Note as distinct from New
// Comment, and Paste Link in Paste Options.

import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  return problems;
}

const ed = (page, fn, arg) => page.evaluate(fn, arg);

/// The menu's rows as a person reads them: buttons, the Paste Options heading,
/// and `—` for each separator. Submenu rows read as their label.
const rows = (page) => page.evaluate(() => {
  const m = document.getElementById("sheet-ctx");
  if (!m) return null;
  const out = [];
  for (const n of m.children) {
    if (n.classList.contains("menu-sep")) out.push("—");
    else if (n.classList.contains("ctx-heading")) out.push(n.textContent.trim());
    else if (n.tagName === "BUTTON" && !n.hidden) out.push(n.textContent.trim());
  }
  return out;
});

const pasteIcons = (page) => page.evaluate(() =>
  [...document.querySelectorAll("#sheet-ctx .ctx-paste-row button")].map((b) => b.getAttribute("aria-label")));

const openCellMenu = (page, r, c) => ed(page, ([r, c]) => {
  window.opencalcEditor.selectForTest(r, c);
  window.opencalcEditor.cellMenu(200, 120);
}, [r, c]);

test("the cell menu has Excel's items, in Excel's order and wording", async ({ page }) => {
  const problems = await boot(page);
  await openCellMenu(page, 1, 1);
  expect(await rows(page)).toEqual([
    "Cut", "Copy", "Paste Options:", "Paste Special…",
    "—",
    "Insert…", "Delete…", "Clear Contents",
    "—",
    "Filter", "Sort",
    "—",
    "New Comment",
    "—",
    "Format Cells…", "Pick From Drop-down List…", "Define Name…", "Link",
  ]);
  // Paste Link is left out: OpenCalc cannot paste a link.
  expect(await pasteIcons(page)).toEqual(["Paste", "Values", "Formulas", "Transpose", "Formatting"]);
  expect(problems).toEqual([]);
});

test("Insert… and Delete… are the dialogs Ctrl++ and Ctrl+- open", async ({ page }) => {
  await boot(page);
  await openCellMenu(page, 1, 1);
  await page.locator("#sheet-ctx > button", { hasText: /^Insert…$/ }).click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Insert");
  await expect(page.locator(".oc-radio")).toHaveText(["Shift cells right", "Shift cells down", "Entire row", "Entire column"]);
  await page.keyboard.press("Escape");

  await openCellMenu(page, 1, 1);
  await page.locator("#sheet-ctx > button", { hasText: /^Delete…$/ }).click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Delete");
});

test("Paste Options: Values pastes the copied result, not the formula", async ({ page }) => {
  await boot(page);
  await ed(page, () => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "=1+1");
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+c");
  await openCellMenu(page, 0, 2);
  await page.locator('#sheet-ctx .ctx-paste-row button[aria-label="Values"]').click();
  await expect(page.locator("#sheet-ctx")).toHaveCount(0);
  await expect.poll(() => ed(page, () => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 2))).toBe("2");
});

test("Sort and Filter are submenus with Excel's words", async ({ page }) => {
  await boot(page);
  await openCellMenu(page, 1, 1);
  await page.locator("#sheet-ctx > button.has-sub", { hasText: /^Sort$/ }).hover();
  const sort = page.locator("#sheet-ctx .ctx-submenu:not([hidden])");
  await expect(sort.locator("button")).toHaveText(["Sort A to Z", "Sort Z to A", "Custom Sort…"]);
  await page.locator("#sheet-ctx > button.has-sub", { hasText: /^Filter$/ }).hover();
  // No filter is on yet, so there is nothing to clear; the colour and icon
  // variants are left out because OpenCalc cannot filter by colour.
  await expect(page.locator("#sheet-ctx .ctx-submenu:not([hidden]) button")).toHaveText(["Filter by Selected Cell's Value"]);
});

test("Sort names a number's order the way Excel does", async ({ page }) => {
  await boot(page);
  await ed(page, () => window.opencalcEditor.wasmApi().session_set_cell(0, 1, 1, "42"));
  await openCellMenu(page, 1, 1);
  await page.locator("#sheet-ctx > button.has-sub", { hasText: /^Sort$/ }).hover();
  await expect(page.locator("#sheet-ctx .ctx-submenu:not([hidden]) button"))
    .toHaveText(["Sort Smallest to Largest", "Sort Largest to Smallest", "Custom Sort…"]);
});

test("Filter by Selected Cell's Value turns the filter on and keeps that value", async ({ page }) => {
  await boot(page);
  await ed(page, () => {
    const a = window.opencalcEditor.wasmApi();
    for (const [r, v] of [[0, "Fruit"], [1, "apple"], [2, "pear"], [3, "apple"]]) a.session_set_cell(0, r, 0, v);
  });
  await openCellMenu(page, 2, 0);
  await page.locator("#sheet-ctx > button.has-sub", { hasText: /^Filter$/ }).hover();
  await page.locator("#sheet-ctx .ctx-submenu:not([hidden]) button", { hasText: "Filter by Selected Cell's Value" }).click();
  // pear stays; both apples are filtered out.
  await expect.poll(() => ed(page, () => JSON.parse(window.opencalcEditor.wasmApi().session_row_px(0, 1, 3))))
    .toEqual([0, 20, 0]);

  // And the menu now offers to clear it, named for the column's heading.
  await openCellMenu(page, 2, 0);
  await page.locator("#sheet-ctx > button.has-sub", { hasText: /^Filter$/ }).hover();
  await page.locator("#sheet-ctx .ctx-submenu:not([hidden]) button", { hasText: 'Clear Filter From "Fruit"' }).click();
  await expect.poll(() => ed(page, () => JSON.parse(window.opencalcEditor.wasmApi().session_row_px(0, 1, 3))))
    .toEqual([20, 20, 20]);
});

test("Pick From Drop-down List… offers the column's entries, as Alt+Down does", async ({ page }) => {
  await boot(page);
  await ed(page, () => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "pear");
    a.session_set_cell(0, 1, 0, "apple");
  });
  await openCellMenu(page, 2, 0);
  await page.locator("#sheet-ctx > button", { hasText: /^Pick From Drop-down List…$/ }).click();
  await expect(page.locator("#sheet-ctx.dv-menu button")).toHaveText(["apple", "pear"]);
});

test("inside a table the cell menu carries Excel's Table submenu, not Create table…", async ({ page }) => {
  await boot(page);
  await ed(page, () => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "Name");
    a.session_set_cell(0, 1, 0, "x");
    a.session_create_table(0, 0, 0, 1, 0, "Table1", true);
  });
  await openCellMenu(page, 1, 0);
  const r = await rows(page);
  expect(r).toContain("Table");
  expect(r).not.toContain("Create table…");
  await page.locator("#sheet-ctx > button.has-sub", { hasText: /^Table$/ }).hover();
  await expect(page.locator("#sheet-ctx .ctx-submenu:not([hidden]) button")).toHaveText(["Totals Row", "Convert to Range"]);
});

for (const [axis, size] of [["row", "Row Height…"], ["col", "Column Width…"]]) {
  test(`the ${axis === "row" ? "row" : "column"} header menu is Excel's`, async ({ page }) => {
    await boot(page);
    await ed(page, (ax) => {
      const e = window.opencalcEditor;
      if (ax === "row") e.selectRow(3, false); else e.selectColumn(3, false);
      e.headerMenu(ax, 200, 120);
    }, axis);
    expect(await rows(page)).toEqual([
      "Cut", "Copy", "Paste Options:", "Paste Special…",
      "—",
      "Insert", "Delete", "Clear Contents",
      "—",
      "Format Cells…", size, "Hide", "Unhide",
    ]);
  });
}

test("the row header's Insert puts the new rows above, as many as are selected", async ({ page }) => {
  await boot(page);
  await ed(page, () => {
    const e = window.opencalcEditor;
    e.wasmApi().session_set_cell(0, 3, 0, "moved");
    e.selectRow(3, false);
    e.headerMenu("row", 200, 120);
  });
  await page.locator("#sheet-ctx > button", { hasText: /^Insert$/ }).click();
  await expect.poll(() => ed(page, () => window.opencalcEditor.wasmApi().session_cell_input(0, 4, 0))).toBe("moved");
});
