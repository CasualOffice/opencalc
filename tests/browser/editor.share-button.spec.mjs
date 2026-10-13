// The title bar's Share button follows the mode's sharing rule.
//
// Owner, 2026-10-14: "if in that mode sharing is not allowed than remove that
// button basically hide that button". The button was built once at start-up
// from whether `file.share` existed, so a host turning `canShare` off after
// boot left a Share button that could only refuse.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

test("Share is shown only while the mode allows sharing", async ({ page }) => {
  await boot(page);
  const share = page.locator(".rb-share");
  await expect(share, "standalone allows sharing").toBeVisible();

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canShare: false }));
  await expect(share, "a Share button that can only refuse").toBeHidden();

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canShare: true }));
  await expect(share).toBeVisible();
});
