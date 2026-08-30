// The file chords a spreadsheet user arrives already knowing (`TAURI-012`).
//
// `Ctrl+S` worked and was **undocumented** — `showShortcuts()` is the only
// in-app shortcut reference and did not list it, so the one chord nobody opens
// a menu for was also the one the documentation omitted. `Ctrl+O` and `Ctrl+N`
// were not bound at all: the browser's own Open-file and New-window dialogs
// took them, which in a desktop window is plainly wrong and in a tab is still
// not what someone in a spreadsheet means.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
}

test("Ctrl+O opens the file picker rather than the browser's", async ({ page }) => {
  await boot(page);
  const opened = await page.evaluate(() => new Promise((resolve) => {
    const input = document.querySelector("#tb-open");
    input.addEventListener("click", () => resolve(true), { once: true });
    // Dispatched on the canvas, which is where the grid's key handler lives.
    // An earlier version fired at `document` and saw nothing — the listener is
    // on the element, and a `keydown` does not travel downwards.
    const c = document.querySelector("canvas");
    c.focus();
    c.dispatchEvent(new KeyboardEvent("keydown", { key: "o", ctrlKey: true, bubbles: true }));
    setTimeout(() => resolve(false), 600);
  }));
  expect(opened, "Ctrl+O did not reach the editor's own file picker").toBe(true);
});

test("Ctrl+N starts a new workbook", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "keep me"));
  await page.waitForTimeout(200);

  // New is the most destructive verb in the application and confirms first, so
  // the confirmation *is* the evidence the chord arrived. It is a `confirmModal`
  // rather than `window.confirm` — an earlier version of this stubbed the
  // native dialog and saw nothing, because nothing calls it.
  await page.evaluate(() => {
    const c = document.querySelector("canvas");
    c.focus();
    c.dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true, bubbles: true }));
  });
  await page.waitForTimeout(500);
  const asked = await page.evaluate(() =>
    document.body.textContent.includes("Start a new workbook?"));
  expect(asked, "Ctrl+N did nothing — a new workbook was never offered").toBe(true);
});

/// **The one chord nobody opens a menu for was the one the list omitted.**
test("the shortcut list names the file chords", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("help.keyboard-shortcuts"));
  await page.waitForTimeout(400);
  // **Read from the dialog's own rows, not from `document.body`.** The whole
  // page text contains the File menu, which already displays `Ctrl+S` beside
  // Save — so a body-wide search reports the shortcut list as complete when the
  // list itself says nothing. Each `.kb-row` is a label and its chords, which
  // is the thing being asserted about.
  const chords = await page.evaluate(() =>
    [...document.querySelectorAll(".kb-row")].map((r) => r.textContent.replace(/\s+/g, " ").trim()));
  expect(chords.length, "the shortcut dialog did not open").toBeGreaterThan(5);
  const listed = chords.join(" | ");
  for (const chord of ["Ctrl+S", "Ctrl+O", "Ctrl+N"]) {
    // To the end of the chord: `toContain("Ctrl+S")` is satisfied by
    // `Ctrl+Shift+Z`, which is already in this list, so a prefix match reported
    // `Ctrl+S` as documented when it was not.
    const exact = new RegExp(`${chord.replace("+", "\\+")}(?![A-Za-z])`);
    expect(listed, `the shortcut list does not mention ${chord}: ${listed}`).toMatch(exact);
  }
});
