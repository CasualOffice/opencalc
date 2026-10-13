// The Quick Access Toolbar always offers Save where the mode can save at all.
//
// `file.save` is hidden in a browser tab — there is no file to write back to —
// so a QAT built on it lost Save, Excel's first and most-used QAT button. Save
// now takes the first route the mode allows: the file, else a download in the
// opened format, else .xlsx.
import { expect, test } from "@playwright/test";

test("Save is in the Quick Access Toolbar in a browser tab, and saves", async ({ page }) => {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  const save = page.locator(".rb-qat [aria-label=Save]");
  await expect(save).toBeVisible();
  const download = page.waitForEvent("download");
  await save.click();
  expect((await download).suggestedFilename()).toMatch(/\.xlsx$/);
});
