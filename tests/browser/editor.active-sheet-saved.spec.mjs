// The sheet on screen is the sheet a saved file opens on (PIPE-05), as in
// Excel, which writes it as `<workbookView activeTab>` and the sheet's
// `tabSelected`.
//
// The model's `tabSelected` was set by import and by nothing else, so a file
// opened on Sheet1, edited on Sheet3 and saved reopened on Sheet1. Switching
// sheets is not an edit, though: it must not become an undo step or unsaved
// work, any more than it does in Excel.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    a.session_add_sheet(); // Sheet2
    a.session_add_sheet(); // Sheet3
    a.session_set_cell(0, 0, 0, "first");
    a.session_clear_history();
    ed.switchSheet(0);
    ed.renderTabs();
    ed.markSaved();
    ed.draw();
  });
}

const activeTab = (page) => page.locator(".sheet-tab.active").textContent();

test("a file saved on its third sheet reopens on its third sheet", async ({ page }) => {
  await boot(page);
  await page.locator(".sheet-tab", { hasText: "Sheet3" }).click();
  expect(await activeTab(page)).toContain("Sheet3");
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(2, 0, 0, "edited here"));

  const bytes = await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_save_as("xlsx")));
  await page.evaluate((b) => window.opencalcEditor.openBytes(new Uint8Array(b), "saved.xlsx"), bytes);
  await expect(page.locator(".sheet-tab.active")).toContainText("Sheet3");
});

test("switching sheets is neither an undo step nor unsaved work", async ({ page }) => {
  await boot(page);
  const before = await page.evaluate(() => window.opencalcEditor.wasmApi().session_edits_applied());
  await page.locator(".sheet-tab", { hasText: "Sheet2" }).click();
  await page.locator(".sheet-tab", { hasText: "Sheet3" }).click();
  expect(await activeTab(page)).toContain("Sheet3");
  const after = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    return { edits: a.session_edits_applied(), undo: a.session_can_undo(), dirty: ed.isDirty(), active: a.session_active_sheet() };
  });
  expect(after.active, "the engine knows which sheet is open").toBe(2);
  expect(after.edits).toBe(before);
  expect(after.undo).toBe(false);
  expect(after.dirty).toBe(false);
});
