// The confirmation that stands between a user and a shift that quietly breaks
// their formulas.
//
// Inserting or deleting *cells* moves everything below or to the right, and
// this engine does not rewrite the references pointing at what moved. That is a
// deliberate limit; the confirmation is how a user finds out before it happens
// rather than afterwards.
//
// This is a browser gate because `webapp/editor.js` is a browser module and
// there is no other runner for it. It reaches the decision **directly**, rather
// than through the grid's context menu: three earlier attempts at this test
// drove the menu and each failed on locator resolution in the submenu, which
// tested Playwright rather than the editor.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  // The specifier the page itself used — a copy written down here goes stale
  // and imports a second, uninitialised editor.
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// Run the editor's own decision with a probe we control.
const risky = (page, probe) =>
  page.evaluate(async (source) => {
    const editor = await import(window.__editorModule);
    // eslint-disable-next-line no-new-func
    return editor.shiftIsRisky(new Function(source));
  }, probe);

/// **A probe that cannot answer warns, rather than proceeding.**
///
/// This is the defect. The decision started at `false` inside a swallowing
/// `catch`, so a probe that threw asserted the *safe* answer — and the
/// confirmation was skipped for exactly the shift that was about to break
/// formulas. The two wrong answers do not cost the same: guessing "risky"
/// costs one dialog, guessing "safe" costs formulas silently pointing at
/// different cells.
test("a shift whose formula probe throws is confirmed, not performed", async ({ page }) => {
  await boot(page);
  expect(await risky(page, "throw new Error('the engine could not answer')")).toBe(true);
});

/// **A probe that says yes still warns.** The ordinary risky case.
test("a shift the engine says affects formulas is confirmed", async ({ page }) => {
  await boot(page);
  expect(await risky(page, "return true")).toBe(true);
});

/// **A probe that says no does not warn.**
///
/// The control, and the one that stops the fix being "always warn" — which
/// would be safe, useless, and trained away within a day.
test("a shift the engine says is harmless is not confirmed", async ({ page }) => {
  await boot(page);
  expect(await risky(page, "return false")).toBe(false);
});

/// A probe returning something that is neither is coerced, not propagated: the
/// caller branches on it, and `undefined` reaching that branch reads as "safe".
test("a probe returning nothing is treated as harmless, not as a warning", async ({ page }) => {
  await boot(page);
  expect(await risky(page, "return undefined")).toBe(false);
});
