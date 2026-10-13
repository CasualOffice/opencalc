// Alt+Q finds commands by name and runs them (UX-SHELL-15).
import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

test("Alt+Q searches commands, names them briefly, and runs the one chosen", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+q");
  await expect(page.locator(".rb-search-input")).toBeFocused();
  await page.keyboard.type("format");
  const items = page.locator(".rb-search-results .rb-pop-item");
  await expect(items.first()).toBeVisible();
  // A result is a command's name, never its tooltip sentence.
  for (const t of await items.allTextContents()) expect(t, `"${t}" is a sentence, not a name`).not.toMatch(/ — |click to/);
  await page.locator(".rb-search-results .rb-pop-item", { hasText: "Format Cells" }).click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Format Cells");
});
