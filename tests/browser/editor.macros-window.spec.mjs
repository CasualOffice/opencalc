// The Macros window and macros saved in the workbook (`MACRO-02`, docs/99 §3–4).
//
// View ▸ Macros (ONLYOFFICE's place), Developer ▸ Macros and Alt+F8 (Excel's)
// open one window: the workbook's macros, New / Rename / Delete (confirmed) /
// Run / Autostart, and an editor beside the list. The macros are part of the
// workbook — saved as ONLYOFFICE's `xl/jsaProject.bin` — so they survive a
// save and a reopen, and a run is one undo step. A workbook's autostart
// macros are offered on open and run only when the person chooses Run. A
// read-only session shows the macros and no way to change or run them.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_new();
    window.opencalcEditor.forgetMacroAnswersForTest?.();
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}

const modal = (page) => page.locator("#oc-modal");
const rows = (page) => page.locator("#oc-mw-list .oc-mw-row");
const code = (page) => page.locator("#oc-mw-code");
const result = (page) => page.locator("#oc-mw-result");
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const macros = (page) => page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_macros()).macros);

/// Replace the editor's text the way a person would: select all, type.
async function writeCode(page, text) {
  await code(page).click();
  await page.keyboard.press(process.platform === "darwin" ? "Meta+A" : "Control+A");
  await page.keyboard.press("Delete");
  await code(page).fill(text);
}

for (const [how, open] of [
  ["Alt+F8", (p) => p.keyboard.press("Alt+F8")],
  ["View ▸ Macros", async (p) => {
    await p.locator("#rb-tab-view").click();
    await p.locator('#rb-panel-view [data-cmd="view.macros"]').click();
  }],
  ["Developer ▸ Macros", async (p) => {
    await p.locator("#rb-tab-developer").click();
    await p.locator('#rb-panel-developer [data-cmd="developer.macros"]').click();
  }],
]) {
  test(`${how} opens the Macros window`, async ({ page }) => {
    await boot(page);
    await open(page);
    await expect(modal(page)).toBeVisible();
    await expect(page.locator("#oc-modal-title")).toHaveText("Macros");
    await expect(page.locator("#oc-mw-new")).toHaveText("New");
    await expect(page.locator("#oc-mw-rename")).toHaveText("Rename");
    await expect(page.locator("#oc-mw-delete")).toHaveText("Delete");
    await expect(page.locator("#oc-modal .oc-confirm-actions .oc-btn")).toHaveText(["Run", "Close"]);
  });
}

test("the Developer tab carries only what is built", async ({ page }) => {
  await boot(page);
  await page.locator("#rb-tab-developer").click();
  const cmds = await page.locator("#rb-panel-developer [data-cmd]:not([hidden])").evaluateAll((bs) => bs.map((b) => b.dataset.cmd));
  // Excel's Code group: Visual Basic, then Macros — Visual Basic for every workbook.
  expect(cmds, "Record Macro and Convert VBA are later rows, not dead buttons").toEqual(["developer.visual-basic", "developer.macros"]);
});

test("create, edit and run a macro: the cells change and one undo takes the run back", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("Macro 1");
  await expect(code(page)).toHaveValue(/function\(\)/);
  await writeCode(page, [
    "(function () {",
    "  var s = Api.GetActiveSheet();",
    '  s.GetRange("A1").SetValue("hello");',
    '  s.GetRange("B2").SetValue(42);',
    "})();",
  ].join("\n"));
  await page.locator("#oc-mw-run").click();
  await expect(result(page)).toHaveText(/Ran Macro 1: 2 changes, one undo step\./);
  await expect(result(page)).not.toHaveClass(/bad/);
  expect(await input(page, 0, 0)).toBe("hello");
  expect(await input(page, 1, 1)).toBe("42");
  // The source was committed to the workbook before it ran.
  expect((await macros(page))[0].source).toContain('SetValue("hello")');

  // Close, then one Ctrl+Z on the grid takes back the whole run — and only it:
  // the macro stays.
  await page.locator("#oc-mw-close").click();
  await expect(modal(page)).toBeHidden();
  await page.locator("#grid").focus();
  await page.keyboard.press(process.platform === "darwin" ? "Meta+Z" : "Control+Z");
  await expect.poll(() => input(page, 0, 0)).toBe("");
  expect(await input(page, 1, 1)).toBe("");
  expect((await macros(page)).length, "undoing the run does not undo the macro").toBe(1);
});

test("Tab in the editor inserts spaces and Enter a newline", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await writeCode(page, "x");
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Tab");
  await page.keyboard.type("y");
  await expect(code(page)).toHaveValue("x\n    y");
  await expect(modal(page), "Enter did not close the window").toBeVisible();
  await expect(page.locator(".oc-mw-gutter div")).toHaveCount(2);
});

test("a macro error shows inline with its line, and changes nothing", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await writeCode(page, [
    "var s = Api.GetActiveSheet();",
    's.GetRange("A1").SetValue("half");',
    "notDefined();",
  ].join("\n"));
  await page.locator("#oc-mw-run").click();
  await expect(result(page)).toHaveClass(/bad/);
  await expect(result(page)).toHaveText(/^Line 3: .*notDefined/);
  await expect(page.locator(".oc-mw-gutter .bad")).toHaveText("3");
  await expect(modal(page), "inline, not a message box over the dialog").toBeVisible();
  expect(await input(page, 0, 0), "a failed run is all or nothing").toBe("");
});

test("rename in place, and delete only after confirming", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await page.locator("#oc-mw-new").click();
  await expect(rows(page)).toHaveCount(2);

  await rows(page).nth(0).click();
  await page.locator("#oc-mw-rename").click();
  const field = page.locator("#oc-mw-rename-input");
  await expect(field).toBeFocused();
  await field.fill("Tidy up");
  await page.keyboard.press("Enter");
  await expect(modal(page)).toBeVisible();
  await expect(rows(page).nth(0)).toContainText("Tidy up");
  expect((await macros(page)).map((m) => m.name)).toEqual(["Tidy up", "Macro 2"]);

  // Escape abandons a rename.
  await page.locator("#oc-mw-rename").click();
  await page.locator("#oc-mw-rename-input").fill("Nope");
  await page.keyboard.press("Escape");
  await expect(modal(page)).toBeVisible();
  expect((await macros(page))[0].name).toBe("Tidy up");

  // Delete asks; Cancel keeps it and returns to the window.
  await page.locator("#oc-mw-delete").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Macros");
  await expect(page.locator(".oc-confirm-text")).toHaveText(/Delete the macro "Tidy up"\?/);
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn", { hasText: "Cancel" }).click();
  await expect(rows(page)).toHaveCount(2);
  expect((await macros(page)).length).toBe(2);
  // Confirming deletes it.
  await rows(page).nth(0).click();
  await page.locator("#oc-mw-delete").click();
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn", { hasText: "Delete" }).click();
  await expect(rows(page)).toHaveCount(1);
  expect((await macros(page)).map((m) => m.name)).toEqual(["Macro 2"]);
});

/// Save as `.xlsx` and open the bytes again, as a person reopening the file.
async function saveAndReopen(page, name) {
  const bytes = await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_save_as("xlsx")));
  await page.evaluate(([b, n]) => window.opencalcEditor.openBytes(new Uint8Array(b), n), [bytes, name]);
  return bytes;
}

test("a macro survives save and reopen, in ONLYOFFICE's part", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await writeCode(page, 'Api.GetActiveSheet().GetRange("C3").SetValue("kept");');
  await page.locator("#oc-mw-close").click();

  const bytes = await saveAndReopen(page, "kept.xlsx");
  // The package carries ONLYOFFICE's macros part by name.
  expect(Buffer.from(bytes).includes(Buffer.from("xl/jsaProject.bin")), "the part is in the zip").toBe(true);
  const back = await macros(page);
  expect(back.map((m) => [m.name, m.source])).toEqual([["Macro 1", 'Api.GetActiveSheet().GetRange("C3").SetValue("kept");']]);

  await page.locator("#grid").focus();
  await page.keyboard.press("Alt+F8");
  await expect(rows(page)).toHaveCount(1);
  await page.locator("#oc-mw-run").click();
  await expect(result(page)).toHaveText(/Ran Macro 1/);
  expect(await input(page, 2, 2)).toBe("kept");
});

/// A workbook with an autostart macro, saved and reopened.
async function openWithAutostart(page, name = "auto.xlsx") {
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_macro(
      "auto-1", "On open", 'Api.GetActiveSheet().GetRange("A1").SetValue("ran on open");', true);
  });
  await saveAndReopen(page, name);
}

test("autostart asks first, and runs only on Run", async ({ page }) => {
  await boot(page);
  await openWithAutostart(page);
  const barEl = page.locator("#oc-macro-bar");
  await expect(barEl).toBeVisible();
  await expect(barEl).toContainText("This workbook has macros set to run when it opens. Run them?");
  // Asked, not run: nothing has happened yet.
  await page.waitForTimeout(300);
  expect(await input(page, 0, 0)).toBe("");
  await page.locator("#oc-macro-bar-run").click();
  await expect(barEl).toBeHidden();
  await expect.poll(() => input(page, 0, 0)).toBe("ran on open");
});

test("Not now runs nothing, and the answer holds for this document in this tab", async ({ page }) => {
  await boot(page);
  await openWithAutostart(page, "later.xlsx");
  await page.locator("#oc-macro-bar-later").click();
  await expect(page.locator("#oc-macro-bar")).toBeHidden();
  await page.waitForTimeout(300);
  expect(await input(page, 0, 0)).toBe("");
  // Reopening the same document does not ask again, and still runs nothing.
  await saveAndReopen(page, "later.xlsx");
  await expect(page.locator("#oc-macro-bar")).toBeHidden();
  expect(await input(page, 0, 0)).toBe("");
  // A different document is asked about on its own.
  await saveAndReopen(page, "other.xlsx");
  await expect(page.locator("#oc-macro-bar")).toBeVisible();
});

test("autostart is never offered read-only, or where the mode withholds macros", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canRunMacros: false }));
  await openWithAutostart(page, "embedded.xlsx");
  await expect(page.locator("#oc-macro-bar")).toBeHidden();
  expect(await page.evaluate(() => window.opencalcEditor.listCommands().includes("view.macros"))).toBe(false);

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canRunMacros: true }));
  const bytes = await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_save_as("xlsx")));
  // A read-only mode (the viewer's `readOnly` capability, which outlives an
  // open — a bare `setReadOnly` on the session does not, see the report).
  await page.evaluate((b) => {
    window.opencalcEditor.setCapabilities({ readOnly: true });
    window.opencalcEditor.openBytes(new Uint8Array(b), "readonly.xlsx");
  }, bytes);
  expect(await page.evaluate(() => window.opencalcEditor.getCapabilities().readOnly)).toBe(true);
  await expect(page.locator("#oc-macro-bar")).toBeHidden();
  expect(await input(page, 0, 0)).toBe("");
});

test("read-only shows the macros and no way to change or run them", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_macro("g1", "Report", "// read me", false);
    window.opencalcEditor.setReadOnly(true);
  });
  await page.locator("#grid").focus();
  await page.keyboard.press("Alt+F8");
  await expect(modal(page)).toBeVisible();
  await expect(rows(page)).toHaveText(["Report"]);
  await expect(code(page)).toHaveValue("// read me");
  await expect(code(page)).toHaveJSProperty("readOnly", true);
  for (const id of ["#oc-mw-new", "#oc-mw-rename", "#oc-mw-delete", "#oc-mw-run"]) {
    await expect(page.locator(id), `${id} is offered read-only`).toHaveCount(0);
  }
  await expect(page.locator("#oc-mw-autostart")).toBeHidden();
  await expect(page.locator("#oc-modal .oc-confirm-actions .oc-btn")).toHaveText(["Close"]);
  // And the engine refuses an edit whatever the window shows.
  const refused = await page.evaluate(() => {
    try { window.opencalcEditor.wasmApi().session_set_macro_source("g1", "x"); return false; } catch { return true; }
  });
  expect(refused).toBe(true);
});
