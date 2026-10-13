// The status bar carries nothing that is for us (UX-SHELL-05, docs/95 rule 7).
//
// `engine v0.0.0` sat in the status bar of every session. It is a fact about
// the build, not about the document, and Excel shows its version in File ▸
// Account, never beside the cell mode. The version now lives on the status
// element as `data-engine` — which is also the readiness signal the gates wait
// on — and in Help ▸ About.

import { expect, test } from "@playwright/test";

test("no version string is drawn in the status bar", async ({ page }) => {
  await page.goto("/editor.html");
  const status = page.locator("#tb-status");
  await expect(status).toHaveAttribute("data-engine", /^\d+\.\d+\.\d+/, { timeout: 30_000 });
  await expect(status).not.toHaveText(/engine v/);
  await expect(page.locator(".bottom-bar")).not.toContainText(/engine v\d/);
});
