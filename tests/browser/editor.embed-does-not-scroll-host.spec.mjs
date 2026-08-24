// An embedded editor must not move the page it is embedded in.
//
// The landing page puts the live editor in an iframe below the fold. Two
// separate things inside the editor scrolled the *host* document as it booted,
// and between them they pushed the hero — badge, headline, opening paragraph,
// the entire pitch — off the top of the screen. Every visitor arrived at a
// landing page with no headline on it, and the page looked, reasonably enough,
// like a styling problem.
//
// Neither cause is visible from inside the editor, which is why this test lives
// against the landing page rather than the editor page.

import { expect, test } from "@playwright/test";

test("the landing page does not scroll when the embedded editor boots", async ({ page }) => {
  await page.goto("/index.html", { waitUntil: "domcontentloaded" });
  expect(await page.evaluate(() => window.scrollY), "starts at the top").toBe(0);

  // The editor is a WASM boot inside an iframe; give it room to finish.
  await page.waitForTimeout(4000);

  expect(
    await page.evaluate(() => window.scrollY),
    "the page is still at the top after the editor has loaded",
  ).toBe(0);

  // The hero has to be *on screen*, not merely present in the DOM. Asserting on
  // the element alone would have passed throughout the bug.
  const headline = page.locator(".hero h1");
  await expect(headline).toBeInViewport();

  // And the embed must not have taken the keyboard from the host page.
  const active = await page.evaluate(() => document.activeElement?.tagName ?? "");
  expect(active, "the embed waits to be clicked").not.toBe("IFRAME");
});
