// Ctrl+S must save the document, not convert it.
//
// The File menu already knows this. Its first Download entry is named "Same
// format as opened" and carries the reason:
//
//     it is the only one of these that gives back the kind of file that was
//     opened. The others are conversions, and a conversion chosen by accident
//     is how a `.csv` becomes a package under its own name.
//
// Ctrl+S — the one save nobody uses a menu for — called the raw `.xlsx` path.
// It ignored the document's own format, never asked `session_save_loss()`
// (whose binding says the sentence must be said *before* the download, because
// afterwards the file is already on disk), set no status, and had no `catch`:
// inside an async listener a throw became an unhandled rejection, so a failed
// save produced no file and no message.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/** Open a document of a given kind, the way the file picker does. */
async function openAs(page, name, text) {
  await page.evaluate(
    ([name, text]) => {
      const bytes = new TextEncoder().encode(text);
      window.opencalcEditor.openBytes(bytes, name);
    },
    [name, text],
  );
}

test("Ctrl+S gives back the kind of file that was opened", async ({ page }) => {
  await boot(page);
  await openAs(page, "figures.csv", "a,b\n1,2\n");
  await expect
    .poll(() => page.evaluate(() => window.opencalcEditor.wasmApi().session_format()))
    .toBe("csv");

  await page.locator("#grid").focus();
  const download = page.waitForEvent("download");
  await page.keyboard.press("Control+s");

  // The whole point: a .csv saved with Ctrl+S stays a .csv. It used to come
  // back as opencalc.xlsx — a conversion the user never asked for, under a
  // name that hides it.
  expect((await download).suggestedFilename()).toMatch(/\.csv$/);
});

test("Ctrl+S still writes .xlsx for a workbook that is one", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  const download = page.waitForEvent("download");
  await page.keyboard.press("Control+s");
  expect((await download).suggestedFilename()).toMatch(/\.xlsx$/);
});

test("Ctrl+S says that it saved", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  const download = page.waitForEvent("download");
  await page.keyboard.press("Control+s");
  await download;
  // It set no status at all, so the only save most people use gave no sign it
  // had happened.
  await expect(page.locator("#tb-status")).toContainText(/downloaded/i);
});

test("Ctrl+S settles the unsaved-work warning", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 9, 0, "typed"));
  expect(await page.evaluate(() => window.opencalcEditor.isDirty())).toBe(true);

  await page.locator("#grid").focus();
  const download = page.waitForEvent("download");
  await page.keyboard.press("Control+s");
  await download;

  // A save that does not clear the flag leaves the close warning armed forever,
  // which is how a warning stops being read.
  await expect.poll(() => page.evaluate(() => window.opencalcEditor.isDirty())).toBe(false);
});
