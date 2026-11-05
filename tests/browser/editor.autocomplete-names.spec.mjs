// Formula AutoComplete offers the workbook's defined names and tables beside
// its functions (PIPE-10), as Excel's does: a name is listed with its own icon,
// accepting it writes the name with no "(", and a name scoped to another sheet
// is not offered — it would not resolve from here.
//
// The list used to be the function catalogue and custom functions only, so
// typing `=Ra` in a workbook with a name `Rate` offered RADIANS and RAND and
// never the name the user had just defined.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    a.session_add_sheet(); // Sheet2
    a.session_set_name("", -1, "Rate", -1, "=Sheet1!$A$1", "");
    a.session_set_name("", -1, "RangeOnSheet2", 1, "=Sheet2!$B$1", "");
    a.session_set_name("", -1, "Region_North", 0, "=Sheet1!$C$1:$C$4", "");
    a.session_set_cell(0, 0, 0, "0.25");
    a.session_set_cell(0, 3, 0, "Qty");
    a.session_set_cell(0, 4, 0, "3");
    a.session_create_table(0, 3, 0, 4, 0, "RateTable", true);
    ed.switchSheet(0);
    ed.renderTabs();
    ed.draw();
  });
}

const typeAt = async (page, r, c, text) => {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c), [r, c]);
  await page.locator("#grid").focus();
  await page.keyboard.type(text);
};

const names = (page) => page.locator("#ac-menu .ac-item .ac-name").allTextContents();

test("defined names and tables are offered beside functions, each with its own icon", async ({ page }) => {
  await boot(page);
  await typeAt(page, 6, 6, "=RA");
  await expect(page.locator("#ac-menu")).toBeVisible();
  const listed = await names(page);
  expect(listed).toContain("Rate");
  expect(listed).toContain("RateTable");
  expect(listed).toContain("RAND");
  // Kind is shown, not guessed from case.
  await expect(page.locator('#ac-menu .ac-item[data-kind="name"] .ac-name')).toContainText(["Rate"]);
  await expect(page.locator('#ac-menu .ac-item[data-kind="table"] .ac-name')).toContainText(["RateTable"]);
  await expect(page.locator('#ac-menu .ac-item[data-kind="function"] .ac-name').first()).toBeVisible();
  // A name scoped to Sheet2 does not resolve on Sheet1, so it is not offered.
  expect(listed).not.toContain("RangeOnSheet2");
  await page.keyboard.press("Escape");
});

test("accepting a name writes it without a parenthesis, and the formula computes", async ({ page }) => {
  await boot(page);
  await typeAt(page, 6, 6, "=2*Rat");
  await expect(page.locator('#ac-menu .ac-item[data-kind="name"]')).toHaveCount(1);
  // Arrow to the name and accept it with Tab, as Excel does.
  const listed = await names(page);
  for (let i = 0; i < listed.indexOf("Rate"); i++) await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Tab");
  await expect(page.locator("#formula-input")).toHaveValue("=2*Rate");
  await page.keyboard.press("Enter");
  await expect(page.locator("#a11y-6-6")).toHaveText("0.5");
});

test("an underscore in a name keeps it in the list while it is typed", async ({ page }) => {
  await boot(page);
  await typeAt(page, 6, 6, "=SUM(Region_");
  await expect(page.locator("#ac-menu .ac-item .ac-name")).toHaveText(["Region_North"]);
  await page.keyboard.press("Enter");
  await expect(page.locator("#formula-input")).toHaveValue("=SUM(Region_North");
  await page.keyboard.press("Escape");
});

test("a sheet-scoped name is offered on its own sheet", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => { window.opencalcEditor.switchSheet(1); window.opencalcEditor.renderTabs(); });
  await typeAt(page, 0, 0, "=Ra");
  const listed = await names(page);
  expect(listed).toContain("RangeOnSheet2");
  expect(listed).toContain("Rate");
  // Sheet1's own name is out of scope here.
  await page.keyboard.press("Escape");
  await typeAt(page, 0, 0, "=Regi");
  await expect(page.locator("#ac-menu")).toBeHidden();
  await page.keyboard.press("Escape");
});
