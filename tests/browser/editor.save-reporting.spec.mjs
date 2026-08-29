// A save that did not happen must not say it did.
//
// `download()` returns before the desktop shell has raised its panel, and the
// caller ran `markSaved()` straight after it — so a cancelled panel, a failed
// write, or the boot window where the shell still refuses everything cleared
// the dirty bullet and disarmed the close warning while nothing had been
// written. The user was told their work was safe and could then close the
// window (`SAVE-01`).
//
// The three outcomes are deliberately not alike:
//   written  -> markSaved(), and the file's own name is reported
//   cancelled -> still dirty, and nothing is said, because they know
//   failed    -> still dirty, and the failure is named

import { expect, test } from "@playwright/test";

async function boot(page, native) {
  await page.addInitScript((mode) => {
    if (mode === "none") return;
    window.__opencalcNative = {
      save: (bytes, ext) =>
        mode === "ok" ? Promise.resolve(`figures.${ext}`)
        : mode === "cancel" ? Promise.resolve(null)
        : Promise.reject(new Error("disk is full")),
      setDocument: () => Promise.resolve(),
      syncCapabilities: () => Promise.resolve(null),
      publishMenu: () => Promise.resolve(),
      open: () => Promise.resolve(null),
    };
  }, native);
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_new();
    window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "unsaved work");
  });
  await page.waitForTimeout(250);
}

const dirty = (page) => page.evaluate(() => window.opencalcEditor.isDirty());
const save = (page) => page.evaluate(() => window.opencalcEditor.doSaveNative());

test("a write that lands marks the document saved and names the file", async ({ page }) => {
  await boot(page, "ok");
  expect(await dirty(page), "seeded work is unsaved").toBe(true);
  await save(page);
  await page.waitForTimeout(300);
  expect(await dirty(page), "the bytes landed, so the document is saved").toBe(false);
  await expect(page.locator("#tb-status")).toHaveText(/saved figures\./);
});

test("a cancelled save leaves the document dirty and says nothing", async ({ page }) => {
  await boot(page, "cancel");
  await save(page);
  await page.waitForTimeout(300);
  expect(await dirty(page), "cancelling is not saving").toBe(true);
  await expect(page.locator("#tb-status")).not.toHaveText(/saved|downloaded/);
});

test("a failed write leaves the document dirty and names the failure", async ({ page }) => {
  await boot(page, "fail");
  await save(page);
  await page.waitForTimeout(300);
  expect(await dirty(page), "a failed write is not a save").toBe(true);
  await expect(page.locator("#tb-status")).toHaveText(/could not save/);
});
