// The shell is told when the user is typing, wherever they are typing.
//
// `TAURI-018`. A native menu accelerator is consumed before the webview sees
// the key, so the shell releases the chords that collide with typing —
// `Cmd/Ctrl+C`, `V`, `X`, `A`, `Z` — for as long as an edit is open, and the
// editor is what says an edit is open. `TAURI-016` wired that to the *cell*
// editor, because a cell editor was the case in hand.
//
// Every other text field was left out: Find, Replace, the name box, rename, a
// note, the command palette, every dialog input. In those, the shell believed
// nothing was being typed, the native menu ate the chord, and it ran the
// document command — copy in a note copied the selected *cells*, and paste
// pasted them over the grid. Reported from the desktop build, and impossible
// to see in a browser, where there is no native menu to eat anything.
//
// So the assertion is about the signal rather than about the clipboard: the
// clipboard itself is the shell's to implement, and what was wrong here is
// that the shell was never told. A fake bridge records what it was told.

import { expect, test } from "@playwright/test";

/// Records the *value* passed to `setEditing`, which is the whole point here —
/// `editor.native-chrome.spec.mjs` records only the arity, and an arity is the
/// same for `true` and `false`.
const installShell = (page) =>
  page.addInitScript(() => {
    window.__editingReports = [];
    const record = (fn) => (...args) => {
      if (fn === "setEditing") window.__editingReports.push(args[0]);
      return Promise.resolve(null);
    };
    window.__opencalcNative = new Proxy({}, {
      get: (_t, fn) => (typeof fn === "string" ? record(fn) : undefined),
    });
  });

async function boot(page) {
  await installShell(page);
  await page.goto("/editor.html?chrome=native");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => { window.__editingReports.length = 0; });
}

const reports = (page) => page.evaluate(() => window.__editingReports.slice());

test("focusing a text field outside the grid tells the shell an edit is open", async ({ page }) => {
  await boot(page);

  // The name box: an `<input>` with no `type`, which is a text field, and the
  // reason `takesTypedText` counts the empty string rather than testing for
  // `type === "text"`.
  await page.locator("#cell-ref").focus();
  await expect.poll(() => reports(page)).toContain(true);

  await page.evaluate(() => document.getElementById("cell-ref").blur());
  await expect.poll(() => reports(page)).toContain(false);
});

test("focusing a control that takes no typing does not", async ({ page }) => {
  await boot(page);

  // A range input is an `<input>` and takes no text. Releasing the clipboard
  // chords here would put the bug back the other way round: `Cmd+C` over the
  // zoom slider would copy nothing at all instead of the selected cells.
  await page.locator("#zoom-slider").focus();
  await page.waitForTimeout(150);
  expect(await reports(page)).not.toContain(true);
});
