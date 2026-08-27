// The document lives in wasm memory and nowhere else.
//
// There is no autosave and no draft, and there was no `beforeunload`: closing
// the tab, reloading, or pressing Back discarded an hour of work without a
// word. File ▸ New was worse — it replaced the session outright and then
// cleared the undo history, so Ctrl+Z recovered nothing, while merge,
// merge-across, delimited export and a lossy save all confirmed first. The most
// destructive verb in the application was the only one that did not ask.
//
// "Is there unsaved work?" is asked of the engine rather than tallied here. A
// tally in the editor is a list of every mutation it can perform, and this
// repository has twice learned what a rule that enumerates its subjects costs:
// it is one omission from being wrong, and the omission is the write path
// somebody adds last.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const dirty = (page) => page.evaluate(() => window.opencalcEditor.isDirty());
const edits = (page) =>
  page.evaluate(() => window.opencalcEditor.wasmApi().session_edits_applied());

test("a freshly seeded document is not dirty", async ({ page }) => {
  await boot(page);
  // The seeding writes are edits as far as the engine is concerned, so without
  // a baseline a demo sheet nobody touched would warn on close.
  expect(await dirty(page)).toBe(false);
});

test("an edit makes it dirty, and saving makes it clean", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 9, 0, "typed"));
  expect(await dirty(page), "an edit is unsaved work").toBe(true);

  await page.evaluate(() => window.opencalcEditor.markSaved());
  expect(await dirty(page), "and saving settles it").toBe(false);
});

test("the counter rises with edits and never falls", async ({ page }) => {
  await boot(page);
  const before = await edits(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 9, 0, "one");
    a.session_set_cell(0, 9, 1, "two");
  });
  const after = await edits(page);
  expect(after).toBeGreaterThan(before);

  // Undo counts as an edit too, so undoing back to a save point still reports
  // unsaved work. That is the safe direction — a needless warning costs a
  // click, the other mistake costs the document — and it is why this is not
  // the undo stack's depth, which is bounded and would stop rising at its cap.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_undo());
  expect(await edits(page), "undo does not wind the counter back").toBeGreaterThanOrEqual(after);
});

test("File ▸ New asks before discarding unsaved work", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 9, 0, "precious"));

  await page.evaluate(() => window.opencalcEditor.runCommand("file.new"));
  const modal = page.locator(".oc-modal:not([hidden])");
  await expect(modal, "the most destructive verb must ask").toBeVisible();
  await expect(modal).toContainText(/discard/i);

  // Cancelling keeps the document.
  await page.locator(".oc-modal:not([hidden]) button", { hasText: /cancel/i }).click();
  await expect.poll(() =>
    page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 9, 0)),
  ).toBe("precious");
});

test("File ▸ New does not ask when there is nothing to lose", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.new"));
  // A confirmation on a clean document is the dialog everybody clicks through
  // without reading, which is how the one that matters stops being read.
  await expect(page.locator(".oc-modal:not([hidden])")).toHaveCount(0);
});

test("the page warns before unloading unsaved work", async ({ page }) => {
  await boot(page);
  const armed = () =>
    page.evaluate(() => {
      // Dispatched rather than navigated: a real unload cannot be observed from
      // inside the page, but whether the handler cancels it can.
      const e = new Event("beforeunload", { cancelable: true });
      window.dispatchEvent(e);
      return e.defaultPrevented;
    });

  expect(await armed(), "a clean document leaves quietly").toBe(false);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 9, 0, "unsaved"));
  expect(await armed(), "an edited one does not").toBe(true);
});
