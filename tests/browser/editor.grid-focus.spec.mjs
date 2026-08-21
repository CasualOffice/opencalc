// The grid takes the keyboard when the editor opens (UX-FOCUS-01).
//
// Every grid shortcut is bound on the `<canvas id="grid">` element, so all of
// them need that element to hold focus. Nothing focused it at startup: the
// document loaded with focus on `<body>`, and Ctrl/Cmd+A, Ctrl/Cmd+X, Ctrl+C,
// the arrow keys and every other binding did nothing at all until the user
// happened to click a cell first. On macOS Cmd+A then ran the *browser's*
// select-all over the page chrome, which is how it was reported.
//
// A browser test because focus is a property of a live document. `activeElement`
// after a real load is the only thing that answers this; no unit test of the
// handler can, because the handler was never the problem.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const focused = (page) =>
  page.evaluate(() => `${document.activeElement?.tagName}#${document.activeElement?.id}`);

const selection = (page) =>
  page.evaluate(() => window.opencalcEditor.selectionRectForTest());

/// **The grid holds the keyboard as soon as the editor is up.**
test("the grid has keyboard focus on load", async ({ page }) => {
  await boot(page);
  expect(await focused(page)).toBe("CANVAS#grid");
});

/// The consequence, stated as the user meets it: a shortcut works without
/// having to click the grid first.
test("ctrl/cmd+A selects the region without clicking the grid first", async ({ page }) => {
  await boot(page);

  const before = await selection(page);
  expect(before, "the sheet did not open on a single cell").toEqual({ r0: 0, c0: 0, r1: 0, c1: 0 });

  await page.keyboard.press("ControlOrMeta+a");

  const after = await selection(page);
  expect(after, "select-all did nothing, so the grid never had the keyboard").not.toEqual(before);
  expect(after.r1).toBeGreaterThan(0);
  expect(after.c1).toBeGreaterThan(0);
});

/// And cut, the other key that was reported. Asserted through the status line
/// rather than the OS clipboard, which a headless browser will not hand over.
test("ctrl/cmd+X cuts without clicking the grid first", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("ControlOrMeta+x");
  await expect(page.locator("#tb-status")).toHaveText(/^cut/);
});
