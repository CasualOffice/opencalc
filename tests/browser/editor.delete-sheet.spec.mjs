// Deleting a sheet asks first — and is a command the editor admits to having.
//
// Two defects, found together and fixed together:
//
//  1. The tab menu's `Delete` called `session_delete_sheet` on the click. Every
//     cell, chart, table and note on the sheet went, and every formula pointing
//     at it broke, with no question asked — from a menu whose other entries are
//     `Rename` and a colour swatch. File ▸ New already asks before it discards
//     a workbook; this did not.
//  2. That menu was the *only* route, so `listCommands()` had never heard of
//     the verb. A host with its own chrome could not offer Delete Sheet at all,
//     and neither could the native menu builder, which reads the same list.
//
// The confirmation says undo brings the sheet back, and the last test here is
// why that sentence is allowed to be there: `EditOperation::RemoveSheet`
// restores the whole sheet. A dialog that overstates the danger teaches people
// to click through the ones that do not.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "on the first sheet");
    a.session_add_sheet();
    a.session_set_cell(1, 0, 0, "on the second");
    // Through the editor, not only the engine: the tab strip is what a user
    // right-clicks, and `switchSheet` is what makes Sheet2 the active one —
    // which is the sheet `toolbar.delete-sheet` acts on.
    ed.switchSheet(1);
    ed.renderTabs();
  });
  await page.waitForTimeout(200);
}

const sheets = (page) =>
  page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_sheet_names()));

/// The tab menu, opened the way a user opens it.
async function openTabMenu(page, index) {
  const tab = page.locator(".sheet-tab").nth(index);
  await tab.click({ button: "right" });
  await expect(page.locator("#sheet-ctx")).toBeVisible();
}

test("the tab menu's Delete asks before it destroys the sheet", async ({ page }) => {
  await boot(page);
  expect(await sheets(page)).toHaveLength(2);

  await openTabMenu(page, 1);
  await page.locator("#sheet-ctx button.danger").click();

  // The click alone must change nothing.
  await expect(page.locator("#oc-modal")).toBeVisible();
  expect(await sheets(page), "nothing is deleted until the question is answered")
    .toHaveLength(2);
  await expect(page.locator("#oc-modal-title")).toHaveText(/Delete "Sheet2"\?/);

  await page.locator("#oc-modal-body .oc-btn:not(.primary)").click();
  await expect(page.locator("#oc-modal")).toBeHidden();
  expect(await sheets(page), "Cancel means cancel").toHaveLength(2);
});

test("confirming does delete it", async ({ page }) => {
  await boot(page);
  await openTabMenu(page, 1);
  await page.locator("#sheet-ctx button.danger").click();
  await page.locator("#oc-modal-body .oc-btn.primary").click();
  await page.waitForTimeout(250);
  expect(await sheets(page)).toEqual(["Sheet1"]);
});

/// **The message promises undo, so undo is asserted.**
test("undo brings the deleted sheet back, as the message says", async ({ page }) => {
  await boot(page);
  await openTabMenu(page, 1);
  await page.locator("#sheet-ctx button.danger").click();
  await expect(page.locator("#oc-modal-body .oc-confirm-text")).toHaveText(/Undo \(Ctrl\+Z\)/);
  await page.locator("#oc-modal-body .oc-btn.primary").click();
  await page.waitForTimeout(250);
  expect(await sheets(page)).toEqual(["Sheet1"]);

  await page.locator("#grid").focus();
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  expect(await sheets(page), "the sheet is back").toEqual(["Sheet1", "Sheet2"]);
  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(1, 0, 0)),
    "and so is what was on it",
  ).toBe("on the second");
});

test("the verb is a command, and running it asks the same question", async ({ page }) => {
  await boot(page);
  const ids = await page.evaluate(() => window.opencalcEditor.listCommands());
  expect(ids, "a host cannot offer a verb the editor will not name")
    .toContain("toolbar.delete-sheet");

  await page.evaluate(() => window.opencalcEditor.runCommand("toolbar.delete-sheet"));
  await expect(page.locator("#oc-modal")).toBeVisible();
  expect(await sheets(page), "the command asks too, not only the menu").toHaveLength(2);
  // It acts on the active sheet, which `boot` left as the one that was added.
  await expect(page.locator("#oc-modal-title")).toHaveText(/Delete "Sheet2"\?/);
});

test("the last sheet is refused, and is refused before the question", async ({ page }) => {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(200);

  await page.evaluate(() => window.opencalcEditor.runCommand("toolbar.delete-sheet"));
  await page.waitForTimeout(200);
  // Asking and *then* failing would be a dialog talking somebody into a click
  // that cannot work.
  await expect(page.locator("#oc-modal")).toBeHidden();
  await expect(page.locator("#tb-status")).toHaveText(/cannot delete the last sheet/);
  expect(await sheets(page)).toHaveLength(1);
});

/// **A viewer does not get it.**
///
/// `toolbar.delete-sheet` is not on the read-only whitelist, so the rules hide
/// it — and `listCommands()`/`runCommand` must agree about that, or the command
/// has merely been moved somewhere a user cannot see and a script still can.
test("a read-only editor neither lists nor runs it", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.setReadOnly(true));
  expect(await page.evaluate(() => window.opencalcEditor.listCommands()))
    .not.toContain("toolbar.delete-sheet");
  const outcome = await page.evaluate(() => {
    try { window.opencalcEditor.runCommand("toolbar.delete-sheet"); return "ran"; }
    catch (e) { return `refused: ${e.message}`; }
  });
  expect(outcome).toMatch(/^refused:/);
  expect(await sheets(page)).toHaveLength(2);
});

/// **The embed case, which is where this kind of wiring goes wrong.**
///
/// `<opencalc-sheet>` clones `editor.html` into a **shadow root**. A button
/// wired with `document.getElementById` at module scope resolves to `null`
/// there, and the command would then be listed by an editor that cannot run
/// it — listed, because the id is stamped from the markup either way. The
/// wiring goes through `byId`, which resolves against this mount's own root.
test("an embedded editor lists the command and can actually run it", async ({ page }) => {
  await page.goto("/embed.html");
  const el = page.locator("opencalc-sheet#sheet");
  await el.waitFor();
  await page.waitForFunction(
    () => document.querySelector("opencalc-sheet#sheet")?.ready?.then,
    null,
    { timeout: 30_000 },
  );
  await page.evaluate(() => document.querySelector("opencalc-sheet#sheet").ready);

  const named = await page.evaluate(async () =>
    (await document.querySelector("opencalc-sheet#sheet").listCommands())
      .includes("toolbar.delete-sheet"));
  expect(named, "the embed names the command").toBe(true);

  const before = await page.evaluate(async () => {
    const s = document.querySelector("opencalc-sheet#sheet");
    const ed = await s.ready;
    ed.wasmApi().session_add_sheet();
    return JSON.parse(ed.wasmApi().session_sheet_names()).length;
  });
  expect(before).toBe(2);

  await page.evaluate(() => document.querySelector("opencalc-sheet#sheet").run("toolbar.delete-sheet"));
  await page.waitForTimeout(300);
  // The question is asked *inside the shadow root*, which is the whole point:
  // a modal that never appeared would mean the click reached nothing.
  const asked = await page.evaluate(() =>
    !!document.querySelector("opencalc-sheet#sheet").shadowRoot
      .querySelector(".oc-modal:not([hidden])"));
  expect(asked, "the embedded editor's own modal opened").toBe(true);
});
