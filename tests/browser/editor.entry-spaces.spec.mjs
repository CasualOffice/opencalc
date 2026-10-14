// Typed text keeps its spaces, through the editor as through the engine
// (XL-FEAT-128). Excel stores "  abc " as typed; a leading space makes "=" or
// "'" plain text; numbers are read through their spaces.

import { expect, test } from "@playwright/test";

async function typeInto(page, ref, text) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  await page.fill("#formula-input", text);
  await page.press("#formula-input", "Enter");
}
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);

test("typed text keeps its spaces; a leading space makes = text", async ({ page }) => {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await typeInto(page, "B2", "  abc ");
  await typeInto(page, "B3", " 42 ");
  await typeInto(page, "B4", " =SUM(1,2");
  await typeInto(page, "B5", "=SUM(1,2");
  expect(await input(page, 1, 1), "spaces around text are kept").toBe("  abc ");
  expect(await input(page, 2, 1), "a number is read through its spaces").toBe("42");
  expect(await input(page, 3, 1), "a leading space: text, not completed or refused as a formula").toBe(" =SUM(1,2");
  expect(await input(page, 4, 1), "a formula is still completed").toBe("=SUM(1,2)");
});
