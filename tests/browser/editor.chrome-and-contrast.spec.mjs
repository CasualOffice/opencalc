// Two things a user found by running the Docker stack, which no test had asked
// about: the editor stacked its header under the host's, and the grid was hard
// to see.
//
// Both are browser-only claims. "How many headers are on screen" and "can you
// make out the gridlines" are not questions a Rust test can be asked.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// **A host that renders its own header can turn ours off.**
///
/// `<opencalc-sheet>` already hides the header by default — "an embedded editor
/// is the host's product, not ours" — but `casual-calc-host` iframes
/// `editor.html` directly and had no way to say so, so it shipped two headers.
test("a host can hide the editor's header", async ({ page }) => {
  await boot(page);
  await expect(page.locator(".app-header")).toBeVisible();

  await boot(page, "?hide=header");
  await expect(page.locator(".app-header"), "the header is still on screen").toBeHidden();
});

/// **Hiding the header does not remove the readiness signal.**
///
/// `#tb-status` lives inside the header, and both this suite and the host page
/// wait on it. Hiding the region must not take it out of the DOM, or the host
/// hangs forever waiting for an element that will never speak.
test("hiding the header keeps the status element readable", async ({ page }) => {
  await boot(page, "?hide=header");
  // Attached and carrying its text, even though it is not painted.
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/);
});

/// **An unknown region is ignored, not turned into a class.**
///
/// The value arrives on a URL, so anybody who can send a link chooses it.
test("an unknown region name never reaches the class list", async ({ page }) => {
  await boot(page, "?hide=header,evil-class,another");
  const classes = await page.evaluate(() => [...document.documentElement.classList].join(" "));
  expect(classes).toContain("oc-hide-header");
  expect(classes, "an arbitrary value became a class on the document root").not.toContain("evil");
  expect(classes).not.toContain("another");
});

/// **The gridlines are visible enough to make out.**
///
/// Measured, not eyeballed: the default was `#f0f1f4`, 1.13:1 against the
/// canvas — lighter than Google Sheets (1.32:1) and Excel (~1.45:1). WCAG 2.1
/// asks 3:1 for a graphical object you need in order to understand the content,
/// and a spreadsheet's grid is what tells you which cell you are in.
test("gridlines carry enough contrast to be seen", async ({ page }) => {
  await boot(page);
  const ratio = await page.evaluate(() => {
    const read = (name) =>
      getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    const hex = (c) => {
      const m = c.match(/^#?([0-9a-f]{6})$/i);
      if (m) return m[1];
      const rgb = c.match(/(\d+),\s*(\d+),\s*(\d+)/);
      return rgb ? [1, 2, 3].map((i) => (+rgb[i]).toString(16).padStart(2, "0")).join("") : null;
    };
    const lum = (h) => {
      const ch = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
      const l = ch.map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
      return 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
    };
    const g = hex(read("--oc-gridline-color"));
    const b = hex(read("--oc-background-color"));
    if (!g || !b) return null;
    const [hi, lo] = [lum(g), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  });

  expect(ratio, "could not read the colours").not.toBeNull();
  // Not 3:1 — a hairline at 3:1 reads as a heavy table border and fights the
  // data, which is why no spreadsheet ships one. This is the category's own
  // level: at or above Excel's, and above Google Sheets'.
  expect(ratio, `gridlines are ${ratio?.toFixed(2)}:1, fainter than Excel's ~1.45:1`)
    .toBeGreaterThanOrEqual(1.4);
});
