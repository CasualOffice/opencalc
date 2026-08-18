// Excel's paging keys, pressed as Excel documents them.
//
// `Alt+PageDown` / `Alt+PageUp` page the grid sideways. The handler existed and
// was unreachable: it sat inside the Ctrl/Cmd branch, so it answered only to
// Ctrl+Alt+PgUp/PgDn and the binding a user would actually try did nothing
// (`UX-PAGE-01`).
//
// A real key press through the canvas, not a call to the handler — the whole
// defect was *which keys reach it*, so a test that invoked it directly would
// have passed throughout.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
  await page.locator("#grid").click({ position: { x: 120, y: 60 } });
}

const scrollX = (page) =>
  page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    return ed.scrollStateForTest().scrollX;
  });

/// **Alt+PageDown pages right.** The binding Excel documents.
test("Alt+PageDown pages the grid sideways", async ({ page }) => {
  await boot(page);
  const before = await scrollX(page);
  await page.keyboard.press("Alt+PageDown");
  const after = await scrollX(page);
  expect(after, `Alt+PageDown moved scrollX from ${before} to ${after}`).toBeGreaterThan(before);
});

/// **And Alt+PageUp comes back.**
test("Alt+PageUp pages back the other way", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+PageDown");
  await page.keyboard.press("Alt+PageDown");
  const out = await scrollX(page);
  expect(out).toBeGreaterThan(0);
  await page.keyboard.press("Alt+PageUp");
  expect(await scrollX(page)).toBeLessThan(out);
});

/// **PageDown on its own still moves down, not sideways.**
///
/// The control. Paging vertically is the far more common key, and a handler
/// that caught every PageDown would satisfy the tests above while breaking it.
test("PageDown without Alt still pages down rather than across", async ({ page }) => {
  await boot(page);
  const before = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const s = ed.scrollStateForTest();
    return { row: s.row, scrollX: s.scrollX };
  });
  await page.keyboard.press("PageDown");
  const after = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const s = ed.scrollStateForTest();
    return { row: s.row, scrollX: s.scrollX };
  });
  expect(after.row, "PageDown did not move down a page").toBeGreaterThan(before.row);
  expect(after.scrollX, "PageDown paged sideways as well").toBe(before.scrollX);
});
