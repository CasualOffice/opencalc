// Format Cells ▸ Number offers Excel's categories, with the selected cell's
// own value as the sample (UX-SHELL-13).
//
// The tab was a raw format-code field and seven chips, previewing a fixed
// 1234.567 rather than the cell being formatted. Excel users look for
// "Currency" and "2 decimal places"; very few type `$#,##0.00`.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}
const cell = (page, row, col) => page.locator(`#a11y-${row}-${col}`);

test("Currency with two decimals previews the cell and applies", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 3, 1, 3)); // D2 = 13.5
  await page.evaluate(() => window.opencalcEditor.runCommand("format.format-cells"));
  await expect(page.locator("#oc-modal-title")).toHaveText("Format Cells");

  await page.locator('.fc-cat[data-cat="Currency"]').click();
  await expect(page.locator('.fc-cat[data-cat="Currency"]')).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".fc-sample"), "the sample is not the selected cell").toHaveText("$13.50");

  await page.locator(".fc-num-opts input[type=number]").fill("0");
  await expect(page.locator(".fc-sample")).toHaveText("$14");

  await page.locator("#oc-modal .oc-btn.primary").click();
  await expect(cell(page, 1, 3)).toHaveText("$14");
});

test("an existing format opens on its own category", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const e = window.opencalcEditor;
    e.selectForTest(1, 3, 1, 3);
    e.setNumberFormat("0.00%");
    e.runCommand("format.format-cells");
  });
  await expect(page.locator('.fc-cat[data-cat="Percentage"]')).toHaveAttribute("aria-selected", "true");
});

// A registered command has no menu node, so the rules that stamp nodes cannot
// reach it. It is held to the same promise through `commandPolicy` — a
// read-only viewer must not be able to open Format Cells from the ribbon, the
// API or search.
test("read-only takes Format Cells away everywhere", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.setReadOnly(true));
  expect(await page.evaluate(() => window.opencalcEditor.listCommands().includes("format.format-cells"))).toBe(false);
  const refused = await page.evaluate(() => {
    try { window.opencalcEditor.runCommand("format.format-cells"); return "ran"; } catch (e) { return e.message; }
  });
  expect(refused).toMatch(/not available/);
  // Its ribbon route is Home ▸ Cells ▸ Format. Everything that dropdown holds
  // edits the sheet, so in a viewer the button itself goes rather than opening
  // onto an empty or greyed list.
  await expect(page.locator('.oc-ribbon [data-cmd="cells.format"]')).toBeHidden();
});
