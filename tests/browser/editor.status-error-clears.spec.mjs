// An error in the status bar ends with the thing it was about (UX-SHELL-03).
//
// A refused formula wrote its error into the status bar and nothing ever took
// it down: it survived cancelling the edit, moving to another cell, and sat
// beside "Ready" for the rest of the session — two contradicting states at
// once, and a message about a cell the person had long left.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

async function refuse(page) {
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 5, 2, 5));
  // A surplus bracket is still refused after UX-SHELL-02; that is the error.
  await page.keyboard.type("=SUM(B2:B4))");
  await page.keyboard.press("Enter");
  await expect(page.locator("#tb-status .err")).toBeVisible();
}

test("cancelling the edit clears its error", async ({ page }) => {
  await boot(page);
  await refuse(page);
  await page.keyboard.press("Escape");
  await expect(page.locator("#tb-status .err")).toHaveCount(0);
});

test("moving to another cell clears an error about the last one", async ({ page }) => {
  await boot(page);
  await refuse(page);
  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#tb-status .err")).toHaveCount(0);
});

test("the error stays while the edit it describes is still open", async ({ page }) => {
  await boot(page);
  await refuse(page);
  await page.waitForTimeout(300);
  await expect(page.locator("#tb-status .err")).toBeVisible();
});
